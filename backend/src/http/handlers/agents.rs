//! The agent organisation of a workspace.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Deserializer};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::agent::{
    AGENT_NAME_MAX, AGENT_TITLE_MAX, Agent, AgentRuntime, AgentStatus, SYSTEM_PROMPT_MAX,
    is_valid_role,
};
use crate::domain::validation::{FieldErrors, Validate, check_max_len, check_text};
use crate::http::extract::{AuthUser, Path, Query, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo::agents::AgentFields;
use crate::repo::{self, OrNotFound};

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
}

/// `PATCH /agents/{id}` body (any subset; `reports_to: null` detaches).
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
    pub status: Option<AgentStatus>,
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
    }
}

/// Query of `GET /agents`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct AgentQuery {
    /// The workspace whose agents to list (default: the caller's first workspace).
    pub workspace_id: Option<Uuid>,
}

/// The workspace the caller is addressing, after checking they belong to it.
/// Every member may see its agents; changing them takes more than a guest.
async fn workspace_for(
    state: &AppState,
    auth: AuthUser,
    workspace_id: Option<Uuid>,
    write: bool,
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
    if write && !role.is_member() {
        return Err(AppError::Forbidden(
            "guests cannot change a workspace's agents".into(),
        ));
    }
    Ok(workspace_id)
}

/// The workspace of agent `id`, if the caller may change its agents.
async fn workspace_of_agent(state: &AppState, auth: AuthUser, id: Uuid) -> Result<Uuid, AppError> {
    let workspace = repo::agents::workspace_of(&state.db, id)
        .await?
        .flatten()
        .ok_or(AppError::NotFound("agent"))?;
    // An agent of a workspace the caller is not in is as good as missing.
    workspace_for(state, auth, Some(workspace), true)
        .await
        .map_err(|err| match err {
            AppError::NotFound(_) => AppError::NotFound("agent"),
            other => other,
        })
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

/// The agents of a workspace.
#[utoipa::path(get, path = "/agents", tag = "agents", security(("bearer" = [])), params(AgentQuery),
    responses((status = 200, body = [Agent]), (status = 401, body = Problem), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<AgentQuery>,
) -> Result<Json<Vec<Agent>>, AppError> {
    let workspace = workspace_for(&state, auth, query.workspace_id, false).await?;
    Ok(Json(repo::agents::list(&state.db, workspace).await?))
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
    let workspace = workspace_for(&state, auth, req.workspace_id, true).await?;
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
    };
    check_manager(&state, workspace, None, fields.reports_to).await?;
    Ok((
        StatusCode::CREATED,
        Json(repo::agents::create(&state.db, auth.id, workspace, &fields).await?),
    ))
}

/// Updates an agent (members of its workspace and above).
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
    let workspace = workspace_of_agent(&state, auth, id).await?;
    let current = repo::agents::find(&state.db, workspace, id)
        .await
        .or_not_found("agent")?;
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
    };
    check_manager(&state, workspace, Some(id), fields.reports_to).await?;
    Ok(Json(
        repo::agents::update(&state.db, workspace, id, &fields)
            .await
            .or_not_found("agent")?,
    ))
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
    let workspace = workspace_of_agent(&state, auth, id).await?;
    if repo::agents::delete(&state.db, workspace, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound("agent"))
    }
}
