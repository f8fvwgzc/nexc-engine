//! A workspace seen whole: what happened on a day, and how its parts relate.

use axum::Json;
use axum::extract::State;
use chrono::{Duration, NaiveDate, NaiveTime, Utc};
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use super::workspaces::{member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::insight::{
    DaySummary, MapItem, MapKind, MapNeighbourhood, TimelineDay, TimelineEntry, WorkspaceMap,
};
use crate::domain::workspace::WorkspaceAction;
use crate::engine::summary;
use crate::http::extract::{AuthUser, Path, Query};
use crate::http::problem::Problem;
use crate::repo::{self, OrNotFound};

/// Most entries returned for one day.
const DAY_LIMIT: i64 = 1_000;

/// Query of `GET /workspaces/{wid}/timeline`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct TimelineQuery {
    /// The day to show (UTC, `YYYY-MM-DD`); today when left out.
    pub day: Option<NaiveDate>,
}

/// What happened in the workspace on one day (UTC), newest first, across
/// members, teams, issues, graphs, runs, documents and memory. Admins and
/// owners only: it shows every team's work.
#[utoipa::path(get, path = "/workspaces/{wid}/timeline", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), TimelineQuery),
    responses((status = 200, body = [TimelineEntry]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn timeline(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<TimelineQuery>,
) -> Result<Json<Vec<TimelineEntry>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let day = query.day.unwrap_or_else(|| Utc::now().date_naive());
    let (from, to) = summary::bounds(day);
    Ok(Json(
        repo::insight::timeline(&state.db, wid, from, to, DAY_LIMIT).await?,
    ))
}

/// The summary of a day (UTC) that the workspace's model wrote, or `null`
/// when none was asked for yet. `stale` says that more has happened on the
/// day since. Reading it spends no tokens. Admins and owners only.
#[utoipa::path(get, path = "/workspaces/{wid}/timeline/summary", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), TimelineQuery),
    responses((status = 200, body = Option<DaySummary>), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn day_summary(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<TimelineQuery>,
) -> Result<Json<Option<DaySummary>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let day = query.day.unwrap_or_else(|| Utc::now().date_naive());
    Ok(Json(summary::read(&state, wid, day).await?))
}

/// Has the workspace's model summarise a day (UTC) from its timeline and
/// keeps the result, in place of the summary there was. Spends tokens of the
/// caller's or the workspace's AI account, within the workspace's
/// guardrails, and is booked as `summary` usage. Admins and owners only.
#[utoipa::path(post, path = "/workspaces/{wid}/timeline/summary", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), TimelineQuery),
    responses((status = 200, body = DaySummary),
        (status = 403, description = "Not an admin, or refused by the workspace's guardrails", body = Problem),
        (status = 404, body = Problem),
        (status = 409, description = "Nothing happened on that day", body = Problem),
        (status = 422, description = "No AI account to use, or the model failed", body = Problem)))]
pub async fn write_day_summary(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<TimelineQuery>,
) -> Result<Json<DaySummary>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let day = query.day.unwrap_or_else(|| Utc::now().date_naive());
    Ok(Json(summary::write(&state, auth.id, wid, day).await?))
}

/// Query of `GET /workspaces/{wid}/timeline/days`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct DaysQuery {
    /// How many days back to look, 1-366 (default 30).
    pub days: Option<i64>,
}

/// How much happened on each of the last days (UTC), latest first; days on
/// which nothing happened are left out. Admins and owners only.
#[utoipa::path(get, path = "/workspaces/{wid}/timeline/days", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), DaysQuery),
    responses((status = 200, body = [TimelineDay]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn days(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<DaysQuery>,
) -> Result<Json<Vec<TimelineDay>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let back = query.days.unwrap_or(30).clamp(1, 366);
    let tomorrow = (Utc::now().date_naive() + Duration::days(1))
        .and_time(NaiveTime::MIN)
        .and_utc();
    let from = tomorrow - Duration::days(back);
    Ok(Json(
        repo::insight::days(&state.db, wid, from, tomorrow).await?,
    ))
}

/// The relationship map of the workspace: its kinds of things (members,
/// teams, projects, issues, graphs, documents, memories, …) and the ties
/// between them, with today's counts.
#[utoipa::path(get, path = "/workspaces/{wid}/map", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = WorkspaceMap), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn map(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<WorkspaceMap>, AppError> {
    // Counts span private teams, so the map is for those who may see them all.
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    Ok(Json(repo::insight::map(&state.db, wid).await?))
}

/// Query of `GET /workspaces/{wid}/map/{kind}`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct MapQuery {
    /// Part of a name, an e-mail, an identifier.
    pub q: Option<String>,
    /// 1-50, default 20.
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// The things of one kind in the workspace, by name: the members, the
/// teams, the issues, to pick one and follow its ties. Admins and owners
/// only: the list spans private teams.
#[utoipa::path(get, path = "/workspaces/{wid}/map/{kind}", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"),
        ("kind" = MapKind, Path, description = "Kind of thing"), MapQuery),
    responses((status = 200, body = [MapItem]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn map_items(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, kind)): Path<(Uuid, MapKind)>,
    Query(query): Query<MapQuery>,
) -> Result<Json<Vec<MapItem>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let q = query.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let q: Option<String> = q.map(|q| q.chars().take(100).collect());
    let limit = query.limit.unwrap_or(20).clamp(1, 50);
    let offset = query.offset.unwrap_or(0).clamp(0, 1_000_000);
    Ok(Json(
        repo::map::list(&state.db, wid, kind, q.as_deref(), limit, offset).await?,
    ))
}

/// One thing of the workspace with everything it is tied to: a member's
/// teams, issues and graphs; an issue's team, project, people, sub-issues
/// and graph. Each tie gives its count and its first items, whose own ties
/// can be followed in turn. Admins and owners only.
#[utoipa::path(get, path = "/workspaces/{wid}/map/{kind}/{id}", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"),
        ("kind" = MapKind, Path, description = "Kind of thing"),
        ("id" = Uuid, Path, description = "Id of the thing")),
    responses((status = 200, body = MapNeighbourhood), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn map_item(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, kind, id)): Path<(Uuid, MapKind, Uuid)>,
) -> Result<Json<MapNeighbourhood>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    Ok(Json(
        repo::map::neighbourhood(&state.db, wid, kind, id)
            .await
            .or_not_found("thing")?,
    ))
}
