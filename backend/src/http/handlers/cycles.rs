//! Cycles of a team.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::NaiveDate;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::teams::visible_team;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::cycle::{CYCLE_MAX_DAYS, CYCLE_NAME_MAX, CYCLES_MAX, Cycle};
use crate::domain::validation::{FieldErrors, Validate};
use crate::domain::workspace::TeamAccess;
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo::{self, OrNotFound};

/// `POST /workspaces/{wid}/teams/{tid}/cycles` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateCycle {
    /// Optional; a cycle without a name is shown by its number.
    #[serde(default)]
    pub name: String,
    /// First day, included (`YYYY-MM-DD`).
    pub starts_on: NaiveDate,
    /// Last day, included; at most 90 days after the first.
    pub ends_on: NaiveDate,
}

/// `PATCH /workspaces/{wid}/teams/{tid}/cycles/{cid}` body: any subset.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateCycle {
    pub name: Option<String>,
    pub starts_on: Option<NaiveDate>,
    pub ends_on: Option<NaiveDate>,
}

fn check_name(errors: &mut FieldErrors, name: &str) {
    if name.trim().chars().count() > CYCLE_NAME_MAX {
        errors.add("name", "must be at most 60 characters");
    }
}

fn check_dates(starts_on: NaiveDate, ends_on: NaiveDate) -> Result<(), AppError> {
    if ends_on < starts_on {
        return Err(AppError::field("ends_on", "must not be before the start"));
    }
    if (ends_on - starts_on).num_days() >= CYCLE_MAX_DAYS {
        return Err(AppError::field("ends_on", "a cycle lasts at most 90 days"));
    }
    Ok(())
}

impl Validate for CreateCycle {
    fn validate(&self, errors: &mut FieldErrors) {
        check_name(errors, &self.name);
    }
}

impl Validate for UpdateCycle {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Some(name) = &self.name {
            check_name(errors, name);
        }
    }
}

fn require_manage(access: TeamAccess) -> Result<(), AppError> {
    if access.can_manage() {
        Ok(())
    } else {
        Err(AppError::Forbidden(
            "only team owners and workspace admins plan cycles".into(),
        ))
    }
}

fn overlap() -> AppError {
    AppError::Conflict("another cycle of this team covers some of these days".into())
}

/// The cycles of a team, latest first, with how many of their issues are closed.
#[utoipa::path(get, path = "/workspaces/{wid}/teams/{tid}/cycles", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id")),
    responses((status = 200, body = [Cycle]), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid)): Path<(Uuid, Uuid)>,
) -> Result<Json<Vec<Cycle>>, AppError> {
    visible_team(&state, auth, wid, tid).await?;
    Ok(Json(repo::cycles::list(&state.db, tid).await?))
}

/// Plans a cycle (team owners and workspace admins). Cycles of a team do not
/// overlap; the new one gets the team's next cycle number.
#[utoipa::path(post, path = "/workspaces/{wid}/teams/{tid}/cycles", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id")),
    request_body = CreateCycle,
    responses((status = 201, body = Cycle), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Overlaps another cycle", body = Problem), (status = 422, body = Problem)))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<CreateCycle>,
) -> Result<(StatusCode, Json<Cycle>), AppError> {
    require_manage(visible_team(&state, auth, wid, tid).await?.1)?;
    check_dates(req.starts_on, req.ends_on)?;
    if repo::cycles::count(&state.db, tid).await? >= CYCLES_MAX {
        return Err(AppError::Unprocessable(format!(
            "a team has at most {CYCLES_MAX} cycles"
        )));
    }
    let mut tx = state.db.begin().await?;
    let id =
        repo::cycles::create(&mut tx, tid, req.name.trim(), req.starts_on, req.ends_on).await?;
    // Checked after the insert, under the team lock, so two plans cannot both pass.
    if repo::cycles::overlaps(&mut *tx, tid, Some(id), req.starts_on, req.ends_on).await? {
        return Err(overlap());
    }
    tx.commit().await?;
    let cycle = repo::cycles::find(&state.db, tid, id)
        .await
        .or_not_found("cycle")?;
    Ok((StatusCode::CREATED, Json(cycle)))
}

/// Renames or reschedules a cycle.
#[utoipa::path(patch, path = "/workspaces/{wid}/teams/{tid}/cycles/{cid}", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id"),
        ("cid" = Uuid, Path, description = "Cycle id")),
    request_body = UpdateCycle,
    responses((status = 200, body = Cycle), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "Overlaps another cycle", body = Problem), (status = 422, body = Problem)))]
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid, cid)): Path<(Uuid, Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<UpdateCycle>,
) -> Result<Json<Cycle>, AppError> {
    require_manage(visible_team(&state, auth, wid, tid).await?.1)?;
    let cycle = repo::cycles::find(&state.db, tid, cid)
        .await
        .or_not_found("cycle")?;
    let name = req.name.as_deref().map_or(cycle.name.as_str(), str::trim);
    let starts_on = req.starts_on.unwrap_or(cycle.starts_on);
    let ends_on = req.ends_on.unwrap_or(cycle.ends_on);
    check_dates(starts_on, ends_on)?;
    if repo::cycles::overlaps(&state.db, tid, Some(cid), starts_on, ends_on).await? {
        return Err(overlap());
    }
    repo::cycles::save(&state.db, cid, name, starts_on, ends_on).await?;
    Ok(Json(
        repo::cycles::find(&state.db, tid, cid)
            .await
            .or_not_found("cycle")?,
    ))
}

/// Deletes a cycle. Its issues stay, in no cycle.
#[utoipa::path(delete, path = "/workspaces/{wid}/teams/{tid}/cycles/{cid}", tag = "issues", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("tid" = Uuid, Path, description = "Team id"),
        ("cid" = Uuid, Path, description = "Cycle id")),
    responses((status = 204, description = "Deleted"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, tid, cid)): Path<(Uuid, Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require_manage(visible_team(&state, auth, wid, tid).await?.1)?;
    let cycle = repo::cycles::find(&state.db, tid, cid)
        .await
        .or_not_found("cycle")?;
    repo::cycles::delete(&state.db, cycle.id).await?;
    Ok(StatusCode::NO_CONTENT)
}
