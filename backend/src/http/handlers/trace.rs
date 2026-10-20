//! The trace of a run: its spans, and the verification of their hash chain.

use axum::Json;
use axum::extract::State;
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::trace::{self, TracePage, TraceVerification};
use crate::http::extract::{AuthUser, Path, Query};
use crate::http::problem::Problem;
use crate::repo::{self, OrNotFound};

/// Spans per page by default.
pub const TRACE_PAGE_DEFAULT: i64 = 200;
/// Most spans in one page.
pub const TRACE_PAGE_MAX: i64 = 1000;

/// Query of `GET /runs/{rid}/trace`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct TraceQuery {
    /// Return spans whose id is greater than this one (the `next` of the previous page).
    pub after: Option<Uuid>,
    /// Spans per page, 1–1000 (default 200).
    pub limit: Option<i64>,
}

/// The spans of a run in id order (UUID v7: the order they started), paged.
#[utoipa::path(get, path = "/runs/{rid}/trace", tag = "runs", security(("bearer" = [])),
    params(("rid" = Uuid, Path, description = "Run id"), TraceQuery),
    responses((status = 200, body = TracePage), (status = 404, body = Problem)))]
pub async fn trace(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(rid): Path<Uuid>,
    Query(query): Query<TraceQuery>,
) -> Result<Json<TracePage>, AppError> {
    // A deleted graph's runs keep their trace for those who could open the graph.
    repo::runs::find_evidence(&state.db, auth.id, rid)
        .await
        .or_not_found("run")?;
    let limit = query
        .limit
        .unwrap_or(TRACE_PAGE_DEFAULT)
        .clamp(1, TRACE_PAGE_MAX);
    let mut spans = repo::spans::page(&state.db, rid, query.after, limit + 1).await?;
    let next = (spans.len() as i64 > limit).then(|| {
        spans.truncate(limit as usize);
        spans.last().map(|s| s.id)
    });
    Ok(Json(TracePage {
        spans,
        next: next.flatten(),
    }))
}

/// Recomputes the run's hash chain from the stored spans and compares it
/// with what is stored and with the hash sealed when the run finished, and
/// checks the seal's signature with the key published under its `kid`. A
/// deleted graph's runs verify as before.
#[utoipa::path(get, path = "/runs/{rid}/trace/verify", tag = "runs", security(("bearer" = [])),
    params(("rid" = Uuid, Path, description = "Run id")),
    responses((status = 200, body = TraceVerification), (status = 404, body = Problem)))]
pub async fn verify(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(rid): Path<Uuid>,
) -> Result<Json<TraceVerification>, AppError> {
    let (row, _) = repo::runs::find_evidence(&state.db, auth.id, rid)
        .await
        .or_not_found("run")?;
    Ok(Json(verify_run(&state, &row).await?))
}

/// The verification of a run's stored chain and its signed seal.
pub async fn verify_run(
    state: &AppState,
    row: &repo::runs::RunRow,
) -> Result<TraceVerification, AppError> {
    let sealed: Option<[u8; 32]> = row
        .trace_hash
        .as_deref()
        .and_then(|bytes| bytes.try_into().ok());
    let spans = repo::spans::chain(&state.db, row.id).await?;
    // Hashing a long chain is CPU work: keep it off the request threads.
    let verified = tokio::task::spawn_blocking(move || trace::verify(&spans, sealed))
        .await
        .map_err(|err| AppError::Internal(err.into()))?;
    let seal = row.seal.as_ref().map(|seal| {
        let key = state.signing_keys.public_key(&seal.kid);
        seal.check(row.id, verified.chain(), key.as_ref())
    });
    Ok(verified.with_seal(seal))
}
