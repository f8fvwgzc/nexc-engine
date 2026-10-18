//! The latency panel of the Infrastructure page: p50, p99 and max per
//! endpoint over the last five minutes, from this instance's in-process
//! window (every HTTP request by route, every gRPC call this backend made).

use axum::Json;
use axum::extract::State;

use crate::app::AppState;
use crate::domain::AppError;
use crate::http::extract::PlatformAdmin;
use crate::http::problem::Problem;
use crate::observability::latency::LatencyReport;

/// The latency of every endpoint this instance served in the last five
/// minutes, and of the gRPC calls it made (platform administrators).
/// Each instance answers for itself; behind a load balancer the answer
/// is one replica's.
#[utoipa::path(get, path = "/admin/latency", tag = "admin", security(("bearer" = [])),
    responses((status = 200, body = LatencyReport), (status = 403, body = Problem)))]
pub async fn report(
    State(state): State<AppState>,
    _admin: PlatformAdmin,
) -> Result<Json<LatencyReport>, AppError> {
    Ok(Json(state.metrics.latency_window.report()))
}
