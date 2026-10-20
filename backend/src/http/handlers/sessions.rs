//! Sessions: a run as a tree of nodes, agents and sub-agents with tokens
//! and cost rolled up at every level, and the list of a workspace's runs
//! with the same rollups.

use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use super::workspaces::{member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::run::RunStatus;
use crate::domain::session::{self, Session, SessionPage};
use crate::domain::workspace::WorkspaceAction;
use crate::http::extract::{AuthUser, Path, Query};
use crate::http::problem::Problem;
use crate::llm::pricing::cost_usd_cached;
use crate::repo;
use crate::repo::sessions::SessionFilter;

/// Sessions per page by default.
pub const SESSIONS_DEFAULT: i64 = 50;
/// Most sessions in one page.
pub const SESSIONS_MAX: i64 = 200;

/// What a model turn cost, for the rollups.
fn price(model: &str, tokens_in: i64, tokens_out: i64, cached: i64) -> f64 {
    cost_usd_cached(
        model,
        tokens_in.max(0) as u64,
        tokens_out.max(0) as u64,
        cached.max(0) as u64,
    )
}

/// The run as a tree: its nodes, the agent each ran as, the sub-agents it
/// spawned, with status, times, tokens, cost, calls and denials per subtree
/// and the ids to open its spans (whoever may open the run).
#[utoipa::path(get, path = "/runs/{rid}/sessions", tag = "runs", security(("bearer" = [])),
    params(("rid" = Uuid, Path, description = "Run id")),
    responses((status = 200, body = Session), (status = 404, body = Problem)))]
pub async fn tree(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(rid): Path<Uuid>,
) -> Result<Json<Session>, AppError> {
    repo::runs::find_evidence(&state.db, auth.id, rid)
        .await?
        .ok_or(AppError::NotFound("run"))?;
    let run = repo::sessions::run_facts(&state.db, rid)
        .await?
        .ok_or(AppError::NotFound("run"))?;
    let (nodes, spans) = tokio::try_join!(
        repo::sessions::node_facts(&state.db, rid),
        repo::spans::chain(&state.db, rid),
    )?;
    // Hundreds of spans fold in microseconds; thousands are still cheap, but off the request thread.
    let built = tokio::task::spawn_blocking(move || session::build(run, &nodes, &spans, &price))
        .await
        .map_err(|err| AppError::Internal(err.into()))?;
    Ok(Json(built))
}

/// Query of `GET /workspaces/{wid}/sessions`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct SessionQuery {
    /// Only runs this agent worked in.
    pub agent: Option<Uuid>,
    /// Only runs in this state.
    pub status: Option<RunStatus>,
    /// Only runs created at or after this time.
    pub from: Option<DateTime<Utc>>,
    /// Only runs created before this time.
    pub to: Option<DateTime<Utc>>,
    /// Sessions per page, 1–200 (default 50).
    pub limit: Option<i64>,
    /// Only runs older than this one (the `next` of the previous page).
    pub before: Option<Uuid>,
}

/// The workspace's recent runs with their rollups, newest first, narrowed
/// by agent, status and time; paged with `before` (members and above).
#[utoipa::path(get, path = "/workspaces/{wid}/sessions", tag = "runs", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), SessionQuery),
    responses((status = 200, body = SessionPage), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    Query(query): Query<SessionQuery>,
) -> Result<Json<SessionPage>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::ViewMembers,
    )?;
    if let (Some(from), Some(to)) = (query.from, query.to)
        && from >= to
    {
        return Err(AppError::field("to", "must be after `from`"));
    }
    let limit = query
        .limit
        .unwrap_or(SESSIONS_DEFAULT)
        .clamp(1, SESSIONS_MAX);
    let filter = SessionFilter {
        agent_id: query.agent,
        status: query.status.map(RunStatus::as_str),
        from: query.from,
        to: query.to,
        before: query.before,
    };
    let mut sessions = repo::sessions::list(&state.db, auth.id, wid, &filter, limit + 1).await?;
    let next = (sessions.len() as i64 > limit).then(|| {
        sessions.truncate(limit as usize);
        sessions.last().map(|s| s.run_id)
    });
    Ok(Json(SessionPage {
        sessions,
        next: next.flatten(),
    }))
}
