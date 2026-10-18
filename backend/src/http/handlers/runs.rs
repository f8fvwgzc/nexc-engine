//! Runs: start, list, inspect, cancel; stop one sub-agent of a run.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::workspaces::audit;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::audit::AuditAction;
use crate::domain::graph::MAX_NODES;
use crate::domain::run::{Run, RunStatus};
use crate::domain::trace::SpanKind;
use crate::domain::validation::{FieldErrors, Validate, check_max_len};
use crate::engine::scheduler::{self, RunOptions};
use crate::engine::{commands, editor};
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo::audit::Subject;
use crate::repo::{self, OrNotFound};

/// Longest reason given for stopping a sub-agent.
pub const CANCEL_REASON_MAX: usize = 500;

/// `POST /graphs/{gid}/runs` body.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateRun {
    /// Run only these nodes (default: all).
    pub node_ids: Option<Vec<Uuid>>,
    /// Parallel nodes, 1–32 (default `NEXC_MAX_CONCURRENCY`).
    pub max_concurrency: Option<u32>,
    /// Ignore the result cache.
    #[serde(default)]
    pub force: bool,
}

impl Validate for CreateRun {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Some(ids) = &self.node_ids
            && (ids.is_empty() || ids.len() > MAX_NODES as usize)
        {
            errors.add("node_ids", "must contain between 1 and 500 ids");
        }
        if self.max_concurrency.is_some_and(|c| !(1..=32).contains(&c)) {
            errors.add("max_concurrency", "must be between 1 and 32");
        }
    }
}

/// Queues a run; progress streams over SSE (`run.*`, `node.*` events).
#[utoipa::path(post, path = "/graphs/{gid}/runs", tag = "runs", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id"),
        ("Idempotency-Key" = Option<String>, Header, description = "1-255 visible ASCII characters: a retry with the same key and body within 24 hours gets the first response again (`Idempotent-Replayed: true`) instead of a second run")),
    request_body = CreateRun,
    responses(
        (status = 202, description = "Run queued", body = Run),
        (status = 400, description = "Malformed Idempotency-Key", body = Problem),
        (status = 404, body = Problem),
        (status = 409, description = "A request with this Idempotency-Key is still being handled (`Retry-After`)", body = Problem),
        (status = 422, description = "Invalid selection, no LLM API key configured, or an Idempotency-Key used with another request", body = Problem),
    ))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(gid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<CreateRun>,
) -> Result<(StatusCode, Json<Run>), AppError> {
    let opts = RunOptions {
        node_ids: req.node_ids,
        max_concurrency: req.max_concurrency,
        force: req.force,
    };
    Ok((
        StatusCode::ACCEPTED,
        Json(scheduler::create_run(&state, auth.id, gid, opts).await?),
    ))
}

/// Runs of a graph, newest first (at most 100).
#[utoipa::path(get, path = "/graphs/{gid}/runs", tag = "runs", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id")),
    responses((status = 200, body = [Run]), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(gid): Path<Uuid>,
) -> Result<Json<Vec<Run>>, AppError> {
    editor::owned_graph(&state, auth.id, gid).await?;
    let mut conn = state.db.acquire().await?;
    Ok(Json(repo::runs::list_for_graph(&mut conn, gid).await?))
}

/// One run with its node results.
#[utoipa::path(get, path = "/runs/{rid}", tag = "runs", security(("bearer" = [])),
    params(("rid" = Uuid, Path, description = "Run id")),
    responses((status = 200, body = Run), (status = 404, body = Problem)))]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(rid): Path<Uuid>,
) -> Result<Json<Run>, AppError> {
    repo::runs::find_owned(&state.db, auth.id, rid)
        .await
        .or_not_found("run")?;
    Ok(Json(scheduler::load_run(&state, rid).await?))
}

/// Cancels a queued or running run.
#[utoipa::path(post, path = "/runs/{rid}/cancel", tag = "runs", security(("bearer" = [])),
    params(("rid" = Uuid, Path, description = "Run id")),
    responses((status = 200, body = Run), (status = 404, body = Problem)))]
pub async fn cancel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(rid): Path<Uuid>,
) -> Result<Json<Run>, AppError> {
    Ok(Json(scheduler::cancel_run(&state, auth.id, rid).await?))
}

/// `POST /runs/{rid}/agents/{sid}/cancel` body.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelAgentBody {
    /// Why, for the trace and the audit log.
    #[serde(default)]
    pub reason: String,
}

impl Validate for CancelAgentBody {
    fn validate(&self, errors: &mut FieldErrors) {
        check_max_len(errors, "reason", &self.reason, CANCEL_REASON_MAX);
    }
}

/// What stopping a sub-agent recorded.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct AgentCancel {
    pub run_id: Uuid,
    pub node_id: Uuid,
    /// The `spawn` span the sub-agent was born in.
    pub spawn_span_id: Uuid,
    /// The sub-agent's principal.
    pub principal: String,
    pub requested_at: DateTime<Utc>,
    /// Whether the runtime was told at once (the run executes on this instance); otherwise
    /// the instance executing it tells the runtime within a second.
    pub delivered: bool,
}

/// Stops one sub-agent of a running run, named by the `spawn` span it was
/// born in; the run goes on. The runtime cancels the sub-agent's task and its
/// parent sees the tool error "cancelled by an operator"; the stop is a
/// `policy` span of decision `cancelled` on the trace and an
/// `agent_cancelled` entry in the audit log (whoever may cancel the run).
#[utoipa::path(post, path = "/runs/{rid}/agents/{sid}/cancel", tag = "runs", security(("bearer" = [])),
    params(("rid" = Uuid, Path, description = "Run id"), ("sid" = Uuid, Path, description = "The sub-agent's spawn span id")),
    request_body = CancelAgentBody,
    responses((status = 202, description = "The stop is recorded and on its way", body = AgentCancel),
        (status = 404, body = Problem), (status = 409, description = "The run is not running", body = Problem),
        (status = 422, body = Problem)))]
pub async fn cancel_agent(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((rid, sid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<CancelAgentBody>,
) -> Result<(StatusCode, Json<AgentCancel>), AppError> {
    let run = repo::runs::find_owned(&state.db, auth.id, rid)
        .await
        .or_not_found("run")?;
    if run.status != RunStatus::Running {
        return Err(AppError::Conflict(format!(
            "the run is {}; only a running run has agents to stop",
            run.status
        )));
    }
    let spawn = repo::spans::find(&state.db, rid, sid)
        .await?
        .filter(|s| s.kind == SpanKind::Spawn)
        .ok_or(AppError::NotFound("spawn span"))?;
    let principal = spawn
        .attributes
        .get("child")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(&spawn.name)
        .to_owned();
    let reason = req.reason.trim();
    let command = repo::run_commands::insert(
        &state.db,
        repo::run_commands::NewCommand {
            run_id: rid,
            node_id: spawn.node_id,
            command: repo::run_commands::CANCEL_AGENT,
            span_id: sid,
            subject: &principal,
            reason,
            requested_by: auth.id,
        },
    )
    .await?;
    if let Some(workspace) = repo::graphs::workspace_of(&state.db, run.graph_id).await? {
        let detail = format!(
            "run {rid}, node {}, spawn {sid}{}",
            spawn.node_id,
            if reason.is_empty() {
                String::new()
            } else {
                format!(": {reason}")
            }
        );
        audit(
            &state,
            workspace,
            auth.id,
            AuditAction::AgentCancelled,
            Subject::Text(&principal),
            &detail,
        )
        .await;
    }
    // The attempt is open here: the runtime is told now. Elsewhere: within a second.
    let delivered = commands::deliver(&state, rid).await > 0;
    Ok((
        StatusCode::ACCEPTED,
        Json(AgentCancel {
            run_id: rid,
            node_id: spawn.node_id,
            spawn_span_id: sid,
            principal,
            requested_at: command.created_at,
            delivered,
        }),
    ))
}
