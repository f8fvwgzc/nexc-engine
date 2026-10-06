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
use crate::memory::{self, View};
use crate::repo::{self, OrNotFound};

/// Query of `GET /memories`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct MemoryQuery {
    /// The workspace whose memory to read (default: the caller's first workspace).
    pub workspace_id: Option<Uuid>,
    /// Restrict to a graph (plus the caller's user-scope memories).
    pub graph_id: Option<Uuid>,
    /// Hybrid search query; results then carry a `score`.
    pub q: Option<String>,
    /// 1–100, default 20.
    pub limit: Option<u32>,
}

/// Lists (newest first) or searches the memory of a workspace: what its
/// graphs learned, as far as the caller can open those graphs, plus the
/// caller's own notes.
#[utoipa::path(get, path = "/memories", tag = "memories", security(("bearer" = [])), params(MemoryQuery),
    responses((status = 200, body = [Memory]), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<MemoryQuery>,
) -> Result<Json<Vec<Memory>>, AppError> {
    // A graph names its workspace; otherwise the caller does, or their first one is used.
    let workspace = match query.graph_id {
        Some(gid) => {
            editor::owned_graph(&state, auth.id, gid)
                .await?
                .workspace_id
        }
        None => match query.workspace_id {
            Some(id) => {
                repo::workspaces::role_of(&state.db, id, auth.id)
                    .await
                    .or_not_found("workspace")?;
                Some(id)
            }
            None => repo::workspaces::default_for(&state.db, auth.id).await?,
        },
    };
    let Some(workspace) = workspace else {
        return Ok(Json(Vec::new()));
    };
    let view = query.graph_id.map_or(View::All, View::Only);
    let limit = query.limit.unwrap_or(20).clamp(1, 100) as usize;
    let q = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(|q| q.chars().take(500).collect::<String>());
    let (index, db) = (&state.memories, &state.db);
    let memories = match q {
        Some(q) => memory::retrieve(index, db, auth.id, workspace, view, &q, limit).await?,
        None => memory::visible(index, db, auth.id, workspace, view, limit).await?,
    };
    Ok(Json(memories))
}

/// Forgets a memory. Its author may, and so may anyone who can work on the
/// graph it was learned in.
#[utoipa::path(delete, path = "/memories/{id}", tag = "memories", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Memory id")),
    responses((status = 204, description = "Deleted"), (status = 404, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let (author, workspace, graph) = repo::memories::provenance(&state.db, id)
        .await
        .or_not_found("memory")?;
    let allowed = author == auth.id
        || match graph {
            Some(gid) => editor::owned_graph(&state, auth.id, gid).await.is_ok(),
            None => false,
        };
    if !allowed || !repo::memories::delete(&state.db, id).await? {
        return Err(AppError::NotFound("memory"));
    }
    if let Some(workspace) = workspace {
        state.memories.invalidate(workspace);
    }
    Ok(StatusCode::NO_CONTENT)
}
