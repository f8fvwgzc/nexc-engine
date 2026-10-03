//! Orchestrator status.

use axum::Json;
use axum::extract::State;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::status::OrchestratorStatus;
use crate::http::extract::AuthUser;
use crate::http::problem::Problem;
use crate::orchestrator;

/// Queue, running work, agents and backend health for the caller.
#[utoipa::path(get, path = "/orchestrator/status", tag = "orchestrator", security(("bearer" = [])),
    responses((status = 200, body = OrchestratorStatus), (status = 401, body = Problem)))]
pub async fn status(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<OrchestratorStatus>, AppError> {
    Ok(Json(orchestrator::status(&state, auth.id).await?))
}
