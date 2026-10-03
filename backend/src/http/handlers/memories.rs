//! Long-term memory browsing and deletion.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::IntoParams;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::memory::Memory;
use crate::engine::editor;
use crate::http::extract::{AuthUser, Path, Query};
use crate::http::problem::Problem;
use crate::{memory, repo};

/// Query of `GET /memories`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct MemoryQuery {
    /// Restrict to a graph (plus user-scope memories).
    pub graph_id: Option<Uuid>,
    /// Hybrid search query; results then carry a `score`.
    pub q: Option<String>,
    /// 1–100, default 20.
    pub limit: Option<u32>,
}

/// Lists (newest first) or searches the caller's memories.
#[utoipa::path(get, path = "/memories", tag = "memories", security(("bearer" = [])), params(MemoryQuery),
    responses((status = 200, body = [Memory]), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<MemoryQuery>,
) -> Result<Json<Vec<Memory>>, AppError> {
    if let Some(gid) = query.graph_id {
        editor::owned_graph(&state, auth.id, gid).await?;
    }
    let limit = query.limit.unwrap_or(20).clamp(1, 100) as usize;
    let q = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(|q| q.chars().take(500).collect::<String>());
    let memories = match q {
        Some(q) => memory::retrieve(&state.db, auth.id, query.graph_id, &q, limit).await?,
        None => repo::memories::list(&state.db, auth.id, query.graph_id, limit as i64)
            .await?
            .into_iter()
            .map(|m| m.memory)
            .collect(),
    };
    Ok(Json(memories))
}

/// Forgets a memory.
#[utoipa::path(delete, path = "/memories/{id}", tag = "memories", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Memory id")),
    responses((status = 204, description = "Deleted"), (status = 404, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    if repo::memories::delete(&state.db, auth.id, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound("memory"))
    }
}
