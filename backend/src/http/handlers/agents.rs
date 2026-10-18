//! The agent organisation of a workspace: the agents, their ID cards, and
//! taking one out of service.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Deserializer};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use super::workspaces::audit;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::agent::{
    AGENT_NAME_MAX, AGENT_TITLE_MAX, Agent, AgentApiToken, AgentCard, AgentEvidence, AgentRuntime,
    AgentStatus, CARD_OUTPUTS, CARD_SPANS, IssuedAgentApiToken, PURPOSE_MAX, SYSTEM_PROMPT_MAX,
    TOKEN_NAME_MAX, is_valid_role,
};
use crate::domain::audit::AuditAction;
use crate::domain::capabilities::Capabilities;
use crate::domain::trace::Span;
use crate::domain::validation::{FieldErrors, Validate, check_max_len, check_text};
use crate::http::extract::{AuthUser, Path, Query, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo::agents::AgentFields;
use crate::repo::audit::Subject;
use crate::repo::{self, OrNotFound};
use crate::security::api_token;

fn double_option<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

/// `POST /agents` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAgent {
    pub name: String,
    pub role: String,
    #[serde(default)]
    pub title: String,
    pub model: Option<String>,
    #[serde(default)]
    pub system_prompt: String,
    pub reports_to: Option<Uuid>,
    pub budget_tokens: Option<i64>,
    pub runtime: Option<AgentRuntime>,
    /// Workspace to add the agent to (default: the caller's first workspace).
    pub workspace_id: Option<Uuid>,
    /// What the agent is for.
    #[serde(default)]
    pub purpose: String,
    /// The member answerable for the agent (default: the caller).
    pub owner_user_id: Option<Uuid>,
    /// A team of the workspace the agent belongs to.
    pub team_id: Option<Uuid>,
    /// What the agent may do (default: the unrestricted policy).
    pub capabilities: Option<Capabilities>,
}

/// `PATCH /agents/{id}` body (any subset; `reports_to: null` and `team_id: null` detach).
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAgent {
    pub name: Option<String>,
    pub role: Option<String>,
    pub title: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<Uuid>, nullable)]
    pub reports_to: Option<Option<Uuid>>,
    pub budget_tokens: Option<i64>,
    pub runtime: Option<AgentRuntime>,
    /// `active` or `paused`; quarantine has its own routes.
    pub status: Option<AgentStatus>,
    pub purpose: Option<String>,
    pub owner_user_id: Option<Uuid>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<Uuid>, nullable)]
    pub team_id: Option<Option<Uuid>>,
    /// Replaces the whole document; raises the policy version when it differs.
    pub capabilities: Option<Capabilities>,
    /// Publishes the agent as an A2A agent, or withdraws it (workspace admins;
    /// audited as `agent_a2a_changed`; not for an `external` agent).
    pub a2a_exposed: Option<bool>,
}

fn check_name(errors: &mut FieldErrors, name: &str) {
    check_text(errors, "name", name, AGENT_NAME_MAX);
}

fn check_role(errors: &mut FieldErrors, role: &str) {
    if !is_valid_role(role.trim()) {
        errors.add(
            "role",
            "must be lowercase letters, digits, `_` or `-` (max 64)",
        );
    }
}

fn check_budget(errors: &mut FieldErrors, budget: i64) {
    if budget < 0 {
        errors.add("budget_tokens", "must not be negative");
    }
}

impl Validate for CreateAgent {
    fn validate(&self, errors: &mut FieldErrors) {
        check_name(errors, &self.name);
        check_role(errors, &self.role);
        check_max_len(errors, "title", &self.title, AGENT_TITLE_MAX);
        if let Some(model) = &self.model {
            check_text(errors, "model", model, 100);
        }
        check_max_len(
            errors,
            "system_prompt",
            &self.system_prompt,
            SYSTEM_PROMPT_MAX,
        );
        check_budget(errors, self.budget_tokens.unwrap_or_default());
        check_max_len(errors, "purpose", &self.purpose, PURPOSE_MAX);
        if let Some(capabilities) = &self.capabilities {
            capabilities.check(errors);
        }
    }
}

impl Validate for UpdateAgent {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Some(name) = &self.name {
            check_name(errors, name);
        }
        if let Some(role) = &self.role {
            check_role(errors, role);
        }
        if let Some(title) = &self.title {
            check_max_len(errors, "title", title, AGENT_TITLE_MAX);
        }
        if let Some(model) = &self.model {
            check_text(errors, "model", model, 100);
        }
        if let Some(prompt) = &self.system_prompt {
            check_max_len(errors, "system_prompt", prompt, SYSTEM_PROMPT_MAX);
        }
        check_budget(errors, self.budget_tokens.unwrap_or_default());
        if let Some(purpose) = &self.purpose {
            check_max_len(errors, "purpose", purpose, PURPOSE_MAX);
        }
        if self.status == Some(AgentStatus::Quarantined) {
            errors.add("status", "quarantine with POST /agents/{id}/quarantine");
        }
        if let Some(capabilities) = &self.capabilities {
            capabilities.check(errors);
        }
    }
}

/// Query of `GET /agents`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct AgentQuery {
    /// The workspace whose agents to list (default: the caller's first workspace).
    pub workspace_id: Option<Uuid>,
}

/// What a caller wants to do with a workspace's agents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Access {
    /// Every member may see the agents.
    Read,
    /// Changing them takes more than a guest.
    Write,
    /// Taking one out of service takes an admin.
    Admin,
}

/// The workspace the caller is addressing, after checking they belong to it
/// and may do what they are about to.
async fn workspace_for(
    state: &AppState,
    auth: AuthUser,
    workspace_id: Option<Uuid>,
    access: Access,
) -> Result<Uuid, AppError> {
    let workspace_id = match workspace_id {
        Some(id) => id,
        None => repo::workspaces::default_for(&state.db, auth.id)
            .await?
            .ok_or(AppError::NotFound("workspace"))?,
    };
    let role = repo::workspaces::role_of(&state.db, workspace_id, auth.id)
        .await
        .or_not_found("workspace")?;
    match access {
        Access::Read => {}
        Access::Write if !role.is_member() => {
            return Err(AppError::Forbidden(
                "guests cannot change a workspace's agents".into(),
            ));
        }
        Access::Admin if !role.is_admin() => {
            return Err(AppError::Forbidden(
                "only a workspace admin quarantines or releases an agent".into(),
            ));
        }
        Access::Write | Access::Admin => {}
    }
    Ok(workspace_id)
}

/// The workspace of agent `id`, if the caller may do `access` with its agents.
async fn workspace_of_agent(
    state: &AppState,
    auth: AuthUser,
    id: Uuid,
    access: Access,
) -> Result<Uuid, AppError> {
    let workspace = repo::agents::workspace_of(&state.db, id)
        .await?
        .flatten()
        .ok_or(AppError::NotFound("agent"))?;
    // An agent of a workspace the caller is not in is as good as missing.
    workspace_for(state, auth, Some(workspace), access)
        .await
        .map_err(|err| match err {
            AppError::NotFound(_) => AppError::NotFound("agent"),
            other => other,
        })
}

/// The workspace of agent `id`, when the caller may read its agents.
pub(crate) async fn readable_agent(
    state: &AppState,
    auth: AuthUser,
    id: Uuid,
) -> Result<Uuid, AppError> {
    workspace_of_agent(state, auth, id, Access::Read).await
}

/// Validates `reports_to`: an agent of the same workspace, not itself, no cycle.
async fn check_manager(
    state: &AppState,
    workspace: Uuid,
    agent: Option<Uuid>,
    manager: Option<Uuid>,
) -> Result<(), AppError> {
    let Some(mut current) = manager else {
        return Ok(());
    };
    for _ in 0..64 {
        if Some(current) == agent {
            return Err(AppError::field(
                "reports_to",
                "would create a reporting cycle",
            ));
        }
        let m = repo::agents::find(&state.db, workspace, current)
            .await?
            .ok_or_else(|| AppError::field("reports_to", "unknown agent"))?;
        match m.reports_to {
            Some(next) => current = next,
            None => return Ok(()),
        }
    }
    Err(AppError::field("reports_to", "reporting chain too deep"))
}

/// Validates the owner (a member of the workspace) and the team (one of its teams).
async fn check_owner_and_team(
    state: &AppState,
    workspace: Uuid,
    owner: Option<Uuid>,
    team: Option<Uuid>,
) -> Result<(), AppError> {
    if let Some(owner) = owner
        && repo::workspaces::role_of(&state.db, workspace, owner)
            .await?
            .is_none()
    {
        return Err(AppError::field(
            "owner_user_id",
            "must be a member of the workspace",
        ));
    }
    if let Some(team) = team
        && !repo::agents::team_in_workspace(&state.db, workspace, team).await?
    {
        return Err(AppError::field("team_id", "unknown team"));
    }
    Ok(())
}

/// Spans in an agent's activity by default.
pub const ACTIVITY_DEFAULT: i64 = 20;
/// Most spans in an agent's activity.
pub const ACTIVITY_MAX: i64 = 200;

/// Query of `GET /agents/{id}/activity`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct ActivityQuery {
    /// Spans to return, 1–200 (default 20).
    pub limit: Option<i64>,
}

/// What an agent did last, across runs: its most recent spans, newest
/// first. Only spans of runs whose graph the caller may open are listed.
#[utoipa::path(get, path = "/agents/{id}/activity", tag = "agents", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Agent id"), ActivityQuery),
    responses((status = 200, body = [Span]), (status = 404, body = Problem)))]
pub async fn activity(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<Vec<Span>>, AppError> {
    workspace_of_agent(&state, auth, id, Access::Read).await?;
    let limit = query
        .limit
        .unwrap_or(ACTIVITY_DEFAULT)
        .clamp(1, ACTIVITY_MAX);
    Ok(Json(
        repo::spans::activity(&state.db, auth.id, id, limit).await?,
    ))
}

/// The agents of a workspace.
#[utoipa::path(get, path = "/agents", tag = "agents", security(("bearer" = [])), params(AgentQuery),
    responses((status = 200, body = [Agent]), (status = 401, body = Problem), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<AgentQuery>,
) -> Result<Json<Vec<Agent>>, AppError> {
    let workspace = workspace_for(&state, auth, query.workspace_id, Access::Read).await?;
    Ok(Json(repo::agents::list(&state.db, workspace).await?))
}

/// An agent's ID card: who it is, who answers for it, what it may do, what
/// it has left to spend and what it did last (any member of its workspace).
#[utoipa::path(get, path = "/agents/{id}", tag = "agents", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Agent id")),
    responses((status = 200, body = AgentCard), (status = 404, body = Problem)))]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<AgentCard>, AppError> {
    let workspace = workspace_of_agent(&state, auth, id, Access::Read).await?;
    let agent = repo::agents::find(&state.db, workspace, id)
        .await
        .or_not_found("agent")?;
    let owner = repo::agents::owner(&state.db, agent.owner_user_id).await?;
    let team = repo::agents::team(&state.db, agent.team_id).await?;
    let recent_spans = repo::spans::activity(&state.db, auth.id, id, CARD_SPANS).await?;
    let evaluation = repo::agent_evaluations::find(&state.db, id).await?;
    let outputs = repo::runs::evidence_of_agent(&state.db, auth.id, id, CARD_OUTPUTS).await?;
    Ok(Json(AgentCard {
        budget_left: agent.budget_left(),
        agent,
        owner,
        team,
        recent_spans,
        evaluation,
        evidence: AgentEvidence::over(outputs),
    }))
}

/// Adds an agent to a workspace (members and above).
#[utoipa::path(post, path = "/agents", tag = "agents", security(("bearer" = [])), request_body = CreateAgent,
    responses((status = 201, body = Agent), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, body = Problem), (status = 422, body = Problem)))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    ValidatedJson(req): ValidatedJson<CreateAgent>,
) -> Result<(StatusCode, Json<Agent>), AppError> {
    let workspace = workspace_for(&state, auth, req.workspace_id, Access::Write).await?;
    let fields = AgentFields {
        name: req.name.trim().to_owned(),
        role: req.role.trim().to_owned(),
        title: req.title.trim().to_owned(),
        model: req
            .model
            .unwrap_or_else(|| state.settings.llm_model.clone()),
        system_prompt: req.system_prompt,
        reports_to: req.reports_to,
        budget_tokens: req.budget_tokens.unwrap_or(1_000_000),
        runtime: req.runtime.unwrap_or(AgentRuntime::Builtin),
        status: AgentStatus::Active,
        owner_user_id: Some(req.owner_user_id.unwrap_or(auth.id)),
        team_id: req.team_id,
        purpose: req.purpose.trim().to_owned(),
        capabilities: req.capabilities.unwrap_or_default(),
        a2a_exposed: false,
    };
    check_manager(&state, workspace, None, fields.reports_to).await?;
    check_owner_and_team(&state, workspace, fields.owner_user_id, fields.team_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(repo::agents::create(&state.db, auth.id, workspace, &fields).await?),
    ))
}

/// Updates an agent (members of its workspace and above). A change of
/// capabilities raises its policy version and is audited (`policy_changed`).
#[utoipa::path(patch, path = "/agents/{id}", tag = "agents", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Agent id")), request_body = UpdateAgent,
    responses((status = 200, body = Agent), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, body = Problem)))]
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<UpdateAgent>,
) -> Result<Json<Agent>, AppError> {
    let workspace = workspace_of_agent(&state, auth, id, Access::Write).await?;
    let current = repo::agents::find(&state.db, workspace, id)
        .await
        .or_not_found("agent")?;
    if current.status == AgentStatus::Quarantined
        && req.status.is_some_and(|s| s != AgentStatus::Quarantined)
    {
        return Err(AppError::field(
            "status",
            "the agent is quarantined; release it with POST /agents/{id}/release",
        ));
    }
    let policy_changed = req
        .capabilities
        .as_ref()
        .is_some_and(|c| *c != current.capabilities);
    // Publishing an agent over A2A is an admin's decision, and only for an agent that runs here.
    let exposure_changed = req.a2a_exposed.is_some_and(|e| e != current.a2a_exposed);
    if exposure_changed {
        workspace_for(&state, auth, Some(workspace), Access::Admin)
            .await
            .map_err(|err| match err {
                AppError::Forbidden(_) => AppError::Forbidden(
                    "only a workspace admin publishes an agent over A2A or withdraws it".into(),
                ),
                other => other,
            })?;
        if req.a2a_exposed == Some(true)
            && req.runtime.unwrap_or(current.runtime) == AgentRuntime::External
        {
            return Err(AppError::field(
                "a2a_exposed",
                "an external agent runs elsewhere and cannot take A2A tasks here",
            ));
        }
    }
    let fields = AgentFields {
        name: req.name.map_or(current.name, |n| n.trim().to_owned()),
        role: req.role.map_or(current.role, |r| r.trim().to_owned()),
        title: req.title.map_or(current.title, |t| t.trim().to_owned()),
        model: req.model.unwrap_or(current.model),
        system_prompt: req.system_prompt.unwrap_or(current.system_prompt),
        reports_to: req.reports_to.unwrap_or(current.reports_to),
        budget_tokens: req.budget_tokens.unwrap_or(current.budget_tokens),
        runtime: req.runtime.unwrap_or(current.runtime),
        status: req.status.unwrap_or(current.status),
        owner_user_id: req.owner_user_id.or(current.owner_user_id),
        team_id: req.team_id.unwrap_or(current.team_id),
        purpose: req.purpose.map_or(current.purpose, |p| p.trim().to_owned()),
        capabilities: req.capabilities.unwrap_or(current.capabilities),
        a2a_exposed: req.a2a_exposed.unwrap_or(current.a2a_exposed),
    };
    check_manager(&state, workspace, Some(id), fields.reports_to).await?;
    check_owner_and_team(&state, workspace, fields.owner_user_id, fields.team_id).await?;
    let agent = repo::agents::update(&state.db, workspace, id, &fields)
        .await
        .or_not_found("agent")?;
    if policy_changed {
        audit(
            &state,
            workspace,
            auth.id,
            AuditAction::PolicyChanged,
            Subject::Text(&agent.principal_id),
            &format!("policy version {}", agent.policy_version),
        )
        .await;
    }
    if exposure_changed {
        audit(
            &state,
            workspace,
            auth.id,
            AuditAction::AgentA2aChanged,
            Subject::Text(&agent.principal_id),
            if agent.a2a_exposed {
                "published as an A2A agent"
            } else {
                "withdrawn from A2A"
            },
        )
        .await;
    }
    Ok(Json(agent))
}

/// Takes an agent out of service (workspace admins): its running nodes are
/// cancelled, new ones are refused, and its policy version is raised so
/// that no token minted before is honoured again.
#[utoipa::path(post, path = "/agents/{id}/quarantine", tag = "agents", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Agent id")),
    responses((status = 200, body = Agent), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Already quarantined", body = Problem)))]
pub async fn quarantine(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Agent>, AppError> {
    let workspace = workspace_of_agent(&state, auth, id, Access::Admin).await?;
    let agent = repo::agents::quarantine(&state.db, workspace, id)
        .await?
        .ok_or(AppError::Conflict(
            "the agent is already quarantined".into(),
        ))?;
    // Nodes running on this instance stop now; other instances notice within a second.
    let cancelled = state.engine.quarantine_local(id);
    audit(
        &state,
        workspace,
        auth.id,
        AuditAction::AgentQuarantined,
        Subject::Text(&agent.principal_id),
        &format!(
            "policy version {}; {cancelled} running node(s) cancelled here",
            agent.policy_version
        ),
    )
    .await;
    Ok(Json(agent))
}

/// Puts a quarantined agent back in service (workspace admins).
#[utoipa::path(post, path = "/agents/{id}/release", tag = "agents", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Agent id")),
    responses((status = 200, body = Agent), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Not quarantined", body = Problem)))]
pub async fn release(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Agent>, AppError> {
    let workspace = workspace_of_agent(&state, auth, id, Access::Admin).await?;
    let agent = repo::agents::release(&state.db, workspace, id)
        .await?
        .ok_or(AppError::Conflict("the agent is not quarantined".into()))?;
    audit(
        &state,
        workspace,
        auth.id,
        AuditAction::AgentReleased,
        Subject::Text(&agent.principal_id),
        &format!("policy version {}", agent.policy_version),
    )
    .await;
    Ok(Json(agent))
}

/// Removes an agent (its reports no longer report to anyone).
#[utoipa::path(delete, path = "/agents/{id}", tag = "agents", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Agent id")),
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let workspace = workspace_of_agent(&state, auth, id, Access::Write).await?;
    if repo::agents::delete(&state.db, workspace, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound("agent"))
    }
}

// --- API tokens of external agents (the Agent Firewall API) ---------------------------------

/// `POST /agents/{id}/tokens` body.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct IssueToken {
    /// What the token is for, to tell tokens apart.
    #[serde(default)]
    pub name: String,
}

impl Validate for IssueToken {
    fn validate(&self, errors: &mut FieldErrors) {
        check_max_len(errors, "name", &self.name, TOKEN_NAME_MAX);
    }
}

/// The API tokens of an external agent (workspace admins): prefixes and
/// dates, never the tokens.
#[utoipa::path(get, path = "/agents/{id}/tokens", tag = "agents", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Agent id")),
    responses((status = 200, body = [AgentApiToken]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn tokens(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Vec<AgentApiToken>>, AppError> {
    workspace_of_agent(&state, auth, id, Access::Admin).await?;
    Ok(Json(repo::agent_tokens::list(&state.db, id).await?))
}

/// Issues an API token to an external agent (workspace admins). The token
/// is in the answer once and kept as a hash; audited as `agent_token_issued`.
#[utoipa::path(post, path = "/agents/{id}/tokens", tag = "agents", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Agent id")), request_body = IssueToken,
    responses((status = 201, body = IssuedAgentApiToken), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, description = "The agent is not an external one", body = Problem)))]
pub async fn issue_token(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<IssueToken>,
) -> Result<(StatusCode, Json<IssuedAgentApiToken>), AppError> {
    let workspace = workspace_of_agent(&state, auth, id, Access::Admin).await?;
    let agent = repo::agents::find(&state.db, workspace, id)
        .await
        .or_not_found("agent")?;
    if agent.runtime != AgentRuntime::External {
        return Err(AppError::Unprocessable(
            "only an agent with runtime `external` holds an API token".into(),
        ));
    }
    let secret = api_token::generate();
    let token = repo::agent_tokens::insert(
        &state.db,
        id,
        &api_token::hash(&secret),
        &api_token::prefix_of(&secret),
        req.name.trim(),
        auth.id,
    )
    .await?;
    audit(
        &state,
        workspace,
        auth.id,
        AuditAction::AgentTokenIssued,
        Subject::Text(&agent.principal_id),
        &format!("token {} ({})", token.prefix, token.name),
    )
    .await;
    Ok((
        StatusCode::CREATED,
        Json(IssuedAgentApiToken { token, secret }),
    ))
}

/// Revokes an API token (workspace admins); audited as `agent_token_revoked`.
#[utoipa::path(delete, path = "/agents/{id}/tokens/{tid}", tag = "agents", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Agent id"), ("tid" = Uuid, Path, description = "Token id")),
    responses((status = 204, description = "Revoked"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn revoke_token(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((id, tid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    let workspace = workspace_of_agent(&state, auth, id, Access::Admin).await?;
    let agent = repo::agents::find(&state.db, workspace, id)
        .await
        .or_not_found("agent")?;
    let token = repo::agent_tokens::revoke(&state.db, id, tid)
        .await?
        .ok_or(AppError::NotFound("token"))?;
    audit(
        &state,
        workspace,
        auth.id,
        AuditAction::AgentTokenRevoked,
        Subject::Text(&agent.principal_id),
        &format!("token {} ({})", token.prefix, token.name),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
