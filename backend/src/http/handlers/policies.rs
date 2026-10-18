//! Versioned policies of a workspace: the history of each level and the
//! effective policy of an agent.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::workspaces::{audit, member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::audit::AuditAction;
use crate::domain::policy_document::{Policy, PolicyDocument, PolicyScope, diff_summary};
use crate::domain::validation::{FieldErrors, Validate};
use crate::domain::workspace::WorkspaceAction;
use crate::engine::policy::{self as engine, Resolved, levels_for};
use crate::http::extract::{AuthUser, Path, Query, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo;
use crate::repo::audit::Subject;

/// Most versions listed at once.
pub const LIST_MAX: i64 = 200;

/// Query of `GET /workspaces/{wid}/policies`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct PolicyQuery {
    /// Only this level.
    pub scope: Option<PolicyScope>,
    /// Only this team or agent.
    pub scope_id: Option<Uuid>,
    /// Versions to return, 1–200 (default 50).
    pub limit: Option<i64>,
}

/// `POST /workspaces/{wid}/policies` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePolicy {
    pub scope: PolicyScope,
    /// The team or agent; left out at the workspace level.
    pub scope_id: Option<Uuid>,
    pub document: PolicyDocument,
}

impl Validate for CreatePolicy {
    fn validate(&self, errors: &mut FieldErrors) {
        match (self.scope, self.scope_id) {
            (PolicyScope::Workspace, Some(_)) => {
                errors.add("scope_id", "a workspace policy has no scope id");
            }
            (PolicyScope::Team | PolicyScope::Agent, None) => {
                errors.add("scope_id", "names the team or agent the policy is for");
            }
            _ => {}
        }
        self.document.check(errors);
    }
}

/// Query of `GET /workspaces/{wid}/policies/resolve`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct ResolveQuery {
    /// The agent whose effective policy to show.
    pub agent_id: Uuid,
}

/// Every version of the workspace's policies, newest first (admins).
#[utoipa::path(get, path = "/workspaces/{wid}/policies", tag = "policies", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), PolicyQuery),
    responses((status = 200, body = [Policy]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<PolicyQuery>,
) -> Result<Json<Vec<Policy>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let limit = query.limit.unwrap_or(50).clamp(1, LIST_MAX);
    Ok(Json(
        repo::policies::list(&state.db, wid, query.scope, query.scope_id, limit).await?,
    ))
}

/// Adds a version at one level (admins). The document may only narrow the
/// policy the levels above it resolve to; the change is audited as
/// `policy_changed` with what changed since the previous version.
#[utoipa::path(post, path = "/workspaces/{wid}/policies", tag = "policies", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = CreatePolicy,
    responses((status = 201, body = Policy), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, body = Problem)))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<CreatePolicy>,
) -> Result<(StatusCode, Json<Policy>), AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    // The subject of the audit entry, and the team an agent-level policy sits under.
    let (subject, team_id) = match (req.scope, req.scope_id) {
        (PolicyScope::Workspace, _) => ("the workspace".to_owned(), None),
        (PolicyScope::Team, Some(team)) => {
            if !repo::agents::team_in_workspace(&state.db, wid, team).await? {
                return Err(AppError::field("scope_id", "unknown team"));
            }
            (format!("team {team}"), Some(team))
        }
        (PolicyScope::Agent, Some(agent)) => {
            let agent = repo::agents::find(&state.db, wid, agent)
                .await?
                .ok_or_else(|| AppError::field("scope_id", "unknown agent"))?;
            (agent.principal_id, agent.team_id)
        }
        _ => return Err(AppError::field("scope_id", "does not fit the scope")),
    };
    let latest = repo::policies::latest_all(&state.db, wid).await?;
    // The levels above the one being set.
    let above = match req.scope {
        PolicyScope::Workspace => Vec::new(),
        PolicyScope::Team => levels_for(&latest, None, None),
        PolicyScope::Agent => levels_for(&latest, team_id, None),
    };
    let parent = engine::resolve(&above, None);
    let mut errors = FieldErrors::default();
    engine::check_narrowing(&parent, &req.document, &mut errors);
    errors.into_result()?;
    let previous = latest
        .iter()
        .find(|p| p.scope == req.scope && p.scope_id == req.scope_id)
        .map(|p| p.document.clone())
        .unwrap_or_default();
    let policy = repo::policies::insert(
        &state.db,
        wid,
        req.scope,
        req.scope_id,
        &req.document,
        auth.id,
    )
    .await?;
    let detail = format!(
        "{} policy version {}: {}",
        req.scope,
        policy.version,
        diff_summary(&previous, &req.document)
    );
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::PolicyChanged,
        Subject::Text(&subject),
        &detail,
    )
    .await;
    Ok((StatusCode::CREATED, Json(policy)))
}

/// The effective policy of an agent: its capabilities narrowed by every
/// level, the rules beside them, and which level set each rule (members).
#[utoipa::path(get, path = "/workspaces/{wid}/policies/resolve", tag = "policies", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ResolveQuery),
    responses((status = 200, body = Resolved), (status = 404, body = Problem)))]
pub async fn resolve(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<ResolveQuery>,
) -> Result<Json<Resolved>, AppError> {
    member_of(&state, auth, wid).await?;
    let agent = repo::agents::find(&state.db, wid, query.agent_id)
        .await?
        .ok_or(AppError::NotFound("agent"))?;
    let latest = repo::policies::latest_all(&state.db, wid).await?;
    let levels = levels_for(&latest, agent.team_id, Some(agent.id));
    Ok(Json(engine::resolve(&levels, Some(&agent.capabilities))))
}
