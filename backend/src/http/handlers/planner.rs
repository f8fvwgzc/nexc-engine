//! Planner status: the queue, the running work, the agents and the health of
//! the backends the Planner routes nodes to (it was called the orchestrator).

use axum::Json;
use axum::extract::State;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::status::PlannerStatus;
use crate::http::extract::AuthUser;
use crate::http::problem::Problem;
use crate::planner;

/// Queue, running work, agents and backend health for the caller.
#[utoipa::path(get, path = "/planner/status", tag = "planner", security(("bearer" = [])),
    responses((status = 200, body = PlannerStatus), (status = 401, body = Problem)))]
pub async fn status(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<PlannerStatus>, AppError> {
    Ok(Json(planner::status(&state, auth.id).await?))
}

/// The same report at its former address; use `GET /planner/status`.
#[deprecated = "use GET /planner/status"]
#[utoipa::path(get, path = "/orchestrator/status", tag = "planner", security(("bearer" = [])),
    responses((status = 200, body = PlannerStatus), (status = 401, body = Problem)))]
pub async fn status_legacy(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<PlannerStatus>, AppError> {
    Ok(Json(planner::status(&state, auth.id).await?))
}
