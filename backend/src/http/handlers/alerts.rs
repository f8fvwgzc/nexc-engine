//! Alerts: what needs a look, and marking it seen.

use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use super::workspaces::{member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::alert::Alert;
use crate::domain::workspace::WorkspaceAction;
use crate::http::extract::{AuthUser, Path, Query};
use crate::http::problem::Problem;
use crate::repo;

/// Most alerts listed at once.
pub const LIST_MAX: i64 = 200;

/// Query of `GET /workspaces/{wid}/alerts`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct AlertQuery {
    /// Only alerts older than this.
    pub before: Option<DateTime<Utc>>,
    /// Alerts to return, 1–200 (default 50).
    pub limit: Option<i64>,
}

/// The alerts of a workspace, newest first (admins).
#[utoipa::path(get, path = "/workspaces/{wid}/alerts", tag = "alerts", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), AlertQuery),
    responses((status = 200, body = [Alert]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<AlertQuery>,
) -> Result<Json<Vec<Alert>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let limit = query.limit.unwrap_or(50).clamp(1, LIST_MAX);
    Ok(Json(
        repo::alerts::list(&state.db, wid, query.before, limit).await?,
    ))
}

/// Marks an alert seen (admins of its workspace).
#[utoipa::path(post, path = "/alerts/{id}/acknowledge", tag = "alerts", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Alert id")),
    responses((status = 200, body = Alert), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Already acknowledged", body = Problem)))]
pub async fn acknowledge(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Alert>, AppError> {
    let alert = repo::alerts::find(&state.db, id)
        .await?
        .ok_or(AppError::NotFound("alert"))?;
    let workspace = member_of(&state, auth, alert.workspace_id)
        .await
        .map_err(|_| AppError::NotFound("alert"))?;
    require(&workspace, WorkspaceAction::UpdateSettings)?;
    let seen = repo::alerts::acknowledge(&state.db, id, auth.id)
        .await?
        .ok_or(AppError::Conflict(
            "the alert was acknowledged already".into(),
        ))?;
    Ok(Json(seen))
}
