//! Planning: request, inspect and apply plans.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::graph::Graph;
use crate::domain::plan::Plan;
use crate::domain::validation::{FieldErrors, Validate, check_max_len};
use crate::engine::{editor, planner};
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo::{self, OrNotFound};

/// `POST /graphs/{gid}/plan` body.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreatePlan {
    #[serde(default)]
    pub instructions: String,
}

impl Validate for CreatePlan {
    fn validate(&self, errors: &mut FieldErrors) {
        check_max_len(errors, "instructions", &self.instructions, 4_000);
    }
}

/// Starts planning; progress streams over SSE (`plan.*` events).
#[utoipa::path(post, path = "/graphs/{gid}/plan", tag = "plans", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id")), request_body = CreatePlan,
    responses(
        (status = 202, description = "Plan is streaming", body = Plan),
        (status = 404, body = Problem),
        (status = 422, description = "No LLM API key configured", body = Problem),
    ))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(gid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<CreatePlan>,
) -> Result<(StatusCode, Json<Plan>), AppError> {
    let plan = planner::start(&state, auth.id, gid, req.instructions.trim().to_owned()).await?;
    Ok((StatusCode::ACCEPTED, Json(plan)))
}

/// A plan of a graph.
#[utoipa::path(get, path = "/graphs/{gid}/plans/{pid}", tag = "plans", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id"), ("pid" = Uuid, Path, description = "Plan id")),
    responses((status = 200, body = Plan), (status = 404, body = Problem)))]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((gid, pid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Plan>, AppError> {
    editor::owned_graph(&state, auth.id, gid).await?;
    Ok(Json(
        repo::plans::find(&state.db, gid, pid)
            .await
            .or_not_found("plan")?,
    ))
}

/// Applies a ready plan to the graph.
#[utoipa::path(post, path = "/graphs/{gid}/plans/{pid}/apply", tag = "plans", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id"), ("pid" = Uuid, Path, description = "Plan id")),
    responses(
        (status = 200, body = Graph),
        (status = 404, body = Problem),
        (status = 409, description = "Plan is not ready", body = Problem),
    ))]
pub async fn apply(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((gid, pid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Graph>, AppError> {
    Ok(Json(planner::apply(&state, auth.id, gid, pid).await?))
}
