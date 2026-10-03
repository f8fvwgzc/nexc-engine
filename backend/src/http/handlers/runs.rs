//! Runs: start, list, inspect, cancel.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::graph::MAX_NODES;
use crate::domain::run::Run;
use crate::domain::validation::{FieldErrors, Validate};
use crate::engine::editor;
use crate::engine::scheduler::{self, RunOptions};
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo::{self, OrNotFound};

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
    params(("gid" = Uuid, Path, description = "Graph id")), request_body = CreateRun,
    responses(
        (status = 202, description = "Run queued", body = Run),
        (status = 404, body = Problem),
        (status = 422, description = "Invalid selection or no LLM API key configured", body = Problem),
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
