//! Role templates for agents (`docs/AGENTSECOPS.md` §4.6): listing them, and
//! making an agent from one, which sets its capabilities, its own policy
//! version, its knowledge bindings, and asks it its competency questions
//! once with the workspace's provider so its card shows a score.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::workspaces::audit;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::agent::{AGENT_NAME_MAX, Agent, AgentCard, AgentStatus};
use crate::domain::agent_template::{
    self, ANSWER_MAX_TOKENS, AgentEvaluation, AgentTemplate, AgentTemplateSpec, DEMO_RUNTIME_MODEL,
    EvaluatedQuestion, excerpt,
};
use crate::domain::audit::AuditAction;
use crate::domain::policy_document::{PolicyScope, diff_summary};
use crate::domain::settings::LlmProviderKind;
use crate::domain::usage::{UsageEvent, UsagePurpose};
use crate::domain::validation::{FieldErrors, Validate, check_text};
use crate::engine::policy::{self as engine, levels_for};
use crate::engine::{credentials, guardrails, usage};
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;
use crate::llm::pricing::cost_usd;
use crate::llm::{LlmRequest, Message, collect};
use crate::repo;
use crate::repo::agents::AgentFields;
use crate::repo::audit::Subject;

/// `POST /agents/from-template/{name}` body (every field optional).
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FromAgentTemplate {
    /// Workspace to add the agent to (default: the caller's first workspace).
    pub workspace_id: Option<Uuid>,
    /// The agent's name (default: the template's).
    pub name: Option<String>,
    /// The model it runs on (default: the server's); the template pins its allow-list to it.
    pub model: Option<String>,
    /// The member answerable for the agent (default: the caller).
    pub owner_user_id: Option<Uuid>,
    /// A team of the workspace the agent belongs to.
    pub team_id: Option<Uuid>,
    /// Whether to ask the competency questions now (default true).
    pub evaluate: Option<bool>,
}

impl Validate for FromAgentTemplate {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Some(name) = &self.name {
            check_text(errors, "name", name, AGENT_NAME_MAX);
        }
        if let Some(model) = &self.model {
            check_text(errors, "model", model, 100);
        }
    }
}

/// The built-in role templates.
#[utoipa::path(get, path = "/agent-templates", tag = "agents", security(("bearer" = [])),
    responses((status = 200, body = [AgentTemplate]), (status = 401, body = Problem)))]
pub async fn list(_auth: AuthUser) -> Json<Vec<AgentTemplate>> {
    Json(
        agent_template::builtin()
            .iter()
            .map(|t| t.summary())
            .collect(),
    )
}

/// The workspace the caller is adding an agent to, if they are a member of it.
async fn workspace_for(
    state: &AppState,
    auth: AuthUser,
    workspace_id: Option<Uuid>,
) -> Result<Uuid, AppError> {
    let workspace_id = match workspace_id {
        Some(id) => id,
        None => repo::workspaces::default_for(&state.db, auth.id)
            .await?
            .ok_or(AppError::NotFound("workspace"))?,
    };
    let role = repo::workspaces::role_of(&state.db, workspace_id, auth.id)
        .await?
        .ok_or(AppError::NotFound("workspace"))?;
    if !role.is_member() {
        return Err(AppError::Forbidden(
            "guests cannot change a workspace's agents".into(),
        ));
    }
    Ok(workspace_id)
}

/// Asks the agent its competency questions with the workspace's provider
/// and scores the answers. `None` when no provider can be used, or when it
/// does not answer: the agent exists either way.
async fn evaluate(
    state: &AppState,
    user: Uuid,
    workspace: Uuid,
    agent: &Agent,
    template: &AgentTemplateSpec,
) -> Option<AgentEvaluation> {
    let llm = match credentials::resolve(state, user, Some(workspace)).await {
        Ok(llm) if llm.is_usable() => llm,
        Ok(_) => {
            tracing::info!(agent = %agent.principal_id, "no usable provider: the agent is not evaluated");
            return None;
        }
        Err(err) => {
            tracing::warn!(agent = %agent.principal_id, error = %err, "provider not resolved: the agent is not evaluated");
            return None;
        }
    };
    let policy = match guardrails::admit(state, Some(workspace), user, llm.target.provider).await {
        Ok(policy) => policy,
        Err(err) => {
            tracing::warn!(agent = %agent.principal_id, error = %err, "guardrails refused the evaluation");
            return None;
        }
    };
    let mut target = llm.target.clone();
    if target.provider != LlmProviderKind::Demo && !agent.model.is_empty() {
        target.model = agent.model.clone();
    }
    let (provider, model) = (target.provider, target.model.clone());
    let mut questions = Vec::with_capacity(template.questions.len());
    for question in &template.questions {
        let request = LlmRequest {
            target: target.clone(),
            system: policy.scrub(agent.system_prompt.clone()),
            messages: vec![Message::user(question.question.clone())],
            max_tokens: ANSWER_MAX_TOKENS,
            json_schema: None,
            effort: Some("low"),
            cacheable: false,
        };
        let done = match collect(state.llm.stream(request), |_| {}).await {
            Ok(done) => done,
            Err(err) => {
                tracing::warn!(agent = %agent.principal_id, error = %err, "the model did not answer a competency question");
                return None;
            }
        };
        let cost = match provider {
            LlmProviderKind::Demo => 0.0,
            _ => cost_usd(&model, done.usage.input_tokens, done.usage.output_tokens),
        };
        usage::record(
            state,
            UsageEvent {
                workspace_id: Some(workspace),
                user_id: user,
                graph_id: None,
                run_id: None,
                purpose: UsagePurpose::Evaluation,
                provider,
                model: model.clone(),
                credential: llm.scope,
                tokens_in: done.usage.input_tokens as i64,
                tokens_out: done.usage.output_tokens as i64,
                cost_usd: cost,
                context_chars_saved: 0,
                agent_id: Some(agent.id),
                node_id: None,
                span_id: None,
            },
        )
        .await;
        questions.push(EvaluatedQuestion {
            question: question.question.clone(),
            expected: question.expected.clone(),
            passed: question.passes(&done.text),
            answer: excerpt(&done.text),
        });
    }
    Some(AgentEvaluation {
        template: template.id.clone(),
        score: questions.iter().filter(|q| q.passed).count() as i32,
        total: questions.len() as i32,
        model,
        questions,
        evaluated_at: Utc::now(),
    })
}

/// Makes an agent from a role template (members and above): its
/// capabilities and purpose from the template, its model allow-list pinned
/// to the model it is given, its knowledge topics bound where the workspace
/// has them, the template's policy as the agent's version 1 (audited as
/// `policy_changed`), and, with a usable provider, its competency questions
/// asked once and scored on the card. 404 for an unknown template, 409 when
/// an agent of that name exists.
#[utoipa::path(post, path = "/agents/from-template/{name}", tag = "agents", security(("bearer" = [])),
    params(("name" = String, Path, description = "Template id: analyst, researcher or reviewer")),
    request_body = FromAgentTemplate,
    responses((status = 201, body = AgentCard), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, body = Problem), (status = 422, body = Problem)))]
pub async fn instantiate(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(name): Path<String>,
    ValidatedJson(req): ValidatedJson<FromAgentTemplate>,
) -> Result<(StatusCode, Json<AgentCard>), AppError> {
    let template = agent_template::find(&name).ok_or(AppError::NotFound("agent template"))?;
    let workspace = workspace_for(&state, auth, req.workspace_id).await?;
    let owner = req.owner_user_id.unwrap_or(auth.id);
    if repo::workspaces::role_of(&state.db, workspace, owner)
        .await?
        .is_none()
    {
        return Err(AppError::field(
            "owner_user_id",
            "must be a member of the workspace",
        ));
    }
    if let Some(team) = req.team_id
        && !repo::agents::team_in_workspace(&state.db, workspace, team).await?
    {
        return Err(AppError::field("team_id", "unknown team"));
    }
    let model = req
        .model
        .map(|m| m.trim().to_owned())
        .unwrap_or_else(|| state.settings.llm_model.clone());
    let mut capabilities = template.capabilities.clone();
    if template.pin_model {
        // The allow-list names the model the agent runs on: the one it is given, the one the
        // workspace's provider would use for it, and the offline demo's own name when that
        // is the provider (its runtime reports `nexc-demo` whatever the agent says).
        let mut allowed = vec![model.clone()];
        if let Ok(resolved) = credentials::resolve(&state, auth.id, Some(workspace)).await {
            if !resolved.target.model.is_empty() && !allowed.contains(&resolved.target.model) {
                allowed.push(resolved.target.model.clone());
            }
            if resolved.is_demo() {
                allowed.push(DEMO_RUNTIME_MODEL.to_owned());
            }
        }
        capabilities.models = Some(allowed);
    }
    let topics: Vec<(Uuid, Vec<String>)> = repo::knowledge::topics(&state.db, workspace)
        .await?
        .into_iter()
        .map(|t| {
            let mut terms = t.terms;
            terms.extend(t.label.split(" · ").map(str::to_owned));
            (t.id, terms)
        })
        .collect();
    if let Some(bound) = template.bound_topics(&topics) {
        capabilities.data.topics = Some(bound);
    }
    // The template's policy may only narrow the levels above the agent.
    let latest = repo::policies::latest_all(&state.db, workspace).await?;
    let above = engine::resolve(&levels_for(&latest, req.team_id, None), None);
    let mut errors = FieldErrors::default();
    engine::check_narrowing(&above, &template.policy, &mut errors);
    errors.into_result()?;

    let fields = AgentFields {
        name: req
            .name
            .map_or_else(|| template.name.clone(), |n| n.trim().to_owned()),
        role: template.role.clone(),
        title: template.title.clone(),
        model,
        system_prompt: template.system_prompt.clone(),
        reports_to: None,
        budget_tokens: template.budget_tokens,
        runtime: template.runtime,
        status: AgentStatus::Active,
        owner_user_id: Some(owner),
        team_id: req.team_id,
        purpose: template.purpose.clone(),
        capabilities,
        a2a_exposed: false,
    };
    let mut tx = state.db.begin().await?;
    let agent = repo::agents::create(&mut *tx, auth.id, workspace, &fields).await?;
    let policy = repo::policies::insert(
        &mut *tx,
        workspace,
        PolicyScope::Agent,
        Some(agent.id),
        &template.policy,
        auth.id,
    )
    .await?;
    tx.commit().await?;
    state
        .caches
        .invalidate(crate::app::caches::Key::Policies(workspace));
    audit(
        &state,
        workspace,
        auth.id,
        AuditAction::PolicyChanged,
        Subject::Text(&agent.principal_id),
        &format!(
            "from template {}: agent policy version {}: {}",
            template.id,
            policy.version,
            diff_summary(&Default::default(), &template.policy)
        ),
    )
    .await;

    let evaluation = if req.evaluate.unwrap_or(true) {
        let scored = evaluate(&state, auth.id, workspace, &agent, template).await;
        if let Some(scored) = &scored {
            repo::agent_evaluations::upsert(&state.db, agent.id, scored).await?;
            state
                .caches
                .invalidate(crate::app::caches::Key::Agent(agent.id));
        }
        scored
    } else {
        None
    };
    let owner = repo::agents::owner(&state.db, agent.owner_user_id).await?;
    let team = repo::agents::team(&state.db, agent.team_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(AgentCard {
            budget_left: agent.budget_left(),
            agent,
            owner,
            team,
            recent_spans: Vec::new(),
            evaluation,
        }),
    ))
}
