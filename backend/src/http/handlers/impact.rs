//! Versions and change assessments of issues and projects
//! (`crate::domain::impact`, `crate::engine::impact`).

use std::collections::HashSet;

use axum::Json;
use axum::extract::State;
use uuid::Uuid;

use super::issues::editable;
use super::workspaces::member_of;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::impact::{
    ASSESSMENTS_MAX, Basis, BasisStatus, ChangeAssessment, ImpactEntityKind, IssueVersion,
};
use crate::http::extract::{AuthUser, Path};
use crate::http::problem::Problem;
use crate::repo::{self, OrNotFound};

/// `assessments` as `viewer` may read them: what they say about items the
/// viewer cannot see is left out ([`ChangeAssessment::redacted`]).
async fn redacted(
    state: &AppState,
    viewer: Uuid,
    workspace_id: Uuid,
    assessments: Vec<ChangeAssessment>,
) -> Result<Vec<ChangeAssessment>, AppError> {
    let mut ids: Vec<Uuid> = assessments
        .iter()
        .flat_map(|a| {
            a.impacts
                .iter()
                .map(|i| i.entity_id)
                .chain(a.violations.iter().flat_map(|v| v.entities.iter().copied()))
                .chain(a.replan_root)
        })
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let visible: HashSet<Uuid> = repo::impact::visible(&state.db, viewer, workspace_id, &ids)
        .await?
        .into_iter()
        .collect();
    Ok(assessments
        .into_iter()
        .map(|a| a.redacted(&|id| visible.contains(&id)))
        .collect())
}

/// The assessed changes of an issue and of the items whose changes asked
/// something of it (a rewritten parent, a deleted blocker), newest first (the
/// last 50): how big each change was, the version it moved to, every item it
/// affects with an action each, the plan conflicts it introduced and the
/// recommendation (`no_action`, `notify`, `review`, `partial_replan`,
/// `full_replan`). Items the caller cannot see are left out.
#[utoipa::path(get, path = "/issues/{iid}/assessments", tag = "issues", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id")),
    responses((status = 200, body = [ChangeAssessment]), (status = 404, body = Problem)))]
pub async fn issue_assessments(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(iid): Path<Uuid>,
) -> Result<Json<Vec<ChangeAssessment>>, AppError> {
    let issue = repo::issues::find(&state.db, auth.id, iid)
        .await
        .or_not_found("issue")?;
    let found = repo::impact::assessments(
        &state.db,
        auth.id,
        issue.workspace_id,
        issue.id,
        ASSESSMENTS_MAX,
    )
    .await?;
    Ok(Json(
        redacted(&state, auth.id, issue.workspace_id, found).await?,
    ))
}

/// The assessed changes of a project and of the items whose changes asked
/// something of it, newest first (the last 50), as for an issue.
#[utoipa::path(get, path = "/workspaces/{wid}/projects/{pid}/assessments", tag = "issues",
    security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("pid" = Uuid, Path, description = "Project id")),
    responses((status = 200, body = [ChangeAssessment]), (status = 404, body = Problem)))]
pub async fn project_assessments(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, pid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<ChangeAssessment>>, AppError> {
    member_of(&state, auth, wid).await?;
    repo::issues::find_project(&state.db, auth.id, wid, pid)
        .await
        .or_not_found("project")?;
    let found = repo::impact::assessments(&state.db, auth.id, wid, pid, ASSESSMENTS_MAX).await?;
    Ok(Json(redacted(&state, auth.id, wid, found).await?))
}

/// An issue's version and where its plan stands against what it is part of.
async fn issue_version(state: &AppState, iid: Uuid) -> Result<IssueVersion, AppError> {
    let node = repo::impact::issue(&state.db, iid)
        .await
        .or_not_found("issue")?;
    let current = match node.basis_target() {
        Some((ImpactEntityKind::Issue, id)) => repo::impact::issue(&state.db, id)
            .await?
            .map(|p| (ImpactEntityKind::Issue, id, p.version)),
        Some((ImpactEntityKind::Project, id)) => repo::impact::project(&state.db, id)
            .await?
            .map(|p| (ImpactEntityKind::Project, id, p.version)),
        None => None,
    };
    let basis = match (node.pinned(), current) {
        (Some((_, pinned)), Some((entity_kind, entity_id, current))) => Some(Basis {
            entity_kind,
            entity_id,
            pinned_version: pinned.to_string(),
            current_version: current.to_string(),
            status: BasisStatus::of(pinned, current),
        }),
        _ => None,
    };
    Ok(IssueVersion {
        issue_id: node.id,
        version: node.version.to_string(),
        basis,
    })
}

/// An issue's version, and the version of what it is part of (its parent
/// issue, else its project) that it was planned against next to that one's
/// version now: `incompatible` after a breaking change there, until the issue
/// is re-planned and acknowledged.
#[utoipa::path(get, path = "/issues/{iid}/version", tag = "issues", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id")),
    responses((status = 200, body = IssueVersion), (status = 404, body = Problem)))]
pub async fn get_issue_version(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(iid): Path<Uuid>,
) -> Result<Json<IssueVersion>, AppError> {
    let issue = repo::issues::find(&state.db, auth.id, iid)
        .await
        .or_not_found("issue")?;
    Ok(Json(issue_version(&state, issue.id).await?))
}

/// Records that the issue's plan was redone against the current version of
/// what it is part of: the issue is pinned to it, so its basis is `current`
/// again. Whoever may edit the issue may.
#[utoipa::path(post, path = "/issues/{iid}/version/acknowledge", tag = "issues", security(("bearer" = [])),
    params(("iid" = Uuid, Path, description = "Issue id")),
    responses((status = 200, body = IssueVersion), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn acknowledge_issue_version(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(iid): Path<Uuid>,
) -> Result<Json<IssueVersion>, AppError> {
    let issue = editable(&state, auth, iid).await?;
    repo::impact::repin(&state.db, issue.id).await?;
    Ok(Json(issue_version(&state, issue.id).await?))
}
