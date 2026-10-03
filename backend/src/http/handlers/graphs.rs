//! Graph CRUD, dependency suggestions and analysis.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::graph::{
    EdgeSuggestion, GRAPH_NAME_MAX, GRAPH_TEXT_MAX, Graph, GraphAnalysis, GraphSummary,
};
use crate::domain::validation::{FieldErrors, Validate, check_max_len, check_text};
use crate::engine::{analysis, deps, editor};
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;
use crate::kernel;
use crate::realtime::events::WsMessage;
use crate::repo::{self, OrNotFound};

/// `POST /graphs` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateGraph {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub goal: String,
}

impl Validate for CreateGraph {
    fn validate(&self, errors: &mut FieldErrors) {
        check_text(errors, "name", &self.name, GRAPH_NAME_MAX);
        check_max_len(errors, "description", &self.description, GRAPH_TEXT_MAX);
        check_max_len(errors, "goal", &self.goal, GRAPH_TEXT_MAX);
    }
}

/// `PATCH /graphs/{gid}` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateGraph {
    pub name: Option<String>,
    pub description: Option<String>,
    pub goal: Option<String>,
}

impl Validate for UpdateGraph {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Some(name) = &self.name {
            check_text(errors, "name", name, GRAPH_NAME_MAX);
        }
        if let Some(d) = &self.description {
            check_max_len(errors, "description", d, GRAPH_TEXT_MAX);
        }
        if let Some(g) = &self.goal {
            check_max_len(errors, "goal", g, GRAPH_TEXT_MAX);
        }
    }
}

/// Strong ETag of a graph's JSON representation (C-kernel hash).
fn etag(graph: &Graph) -> Result<String, AppError> {
    let bytes = serde_json::to_vec(graph).map_err(anyhow::Error::from)?;
    Ok(format!(
        "\"{:016x}\"",
        kernel::hash64(&bytes, graph.version as u64)
    ))
}

/// The caller's graphs, most recently updated first.
#[utoipa::path(get, path = "/graphs", tag = "graphs", security(("bearer" = [])),
    responses((status = 200, body = [GraphSummary]), (status = 401, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Vec<GraphSummary>>, AppError> {
    Ok(Json(repo::graphs::list(&state.db, auth.id).await?))
}

/// Creates an empty graph.
#[utoipa::path(post, path = "/graphs", tag = "graphs", security(("bearer" = [])), request_body = CreateGraph,
    responses((status = 201, body = Graph), (status = 422, body = Problem)))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    ValidatedJson(req): ValidatedJson<CreateGraph>,
) -> Result<(StatusCode, Json<Graph>), AppError> {
    let meta = repo::graphs::create(
        &state.db,
        auth.id,
        req.name.trim(),
        &req.description,
        &req.goal,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(meta.into_graph(Vec::new(), Vec::new())),
    ))
}

/// A graph with nodes and edges. Supports `If-None-Match` (304).
#[utoipa::path(get, path = "/graphs/{gid}", tag = "graphs", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id")),
    responses(
        (status = 200, body = Graph, headers(("ETag" = String, description = "Strong entity tag"))),
        (status = 304, description = "Not modified"),
        (status = 404, body = Problem),
    ))]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(gid): Path<Uuid>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let graph = editor::load_graph(&state, auth.id, gid).await?;
    let tag = etag(&graph)?;
    let tag_value = HeaderValue::from_str(&tag).map_err(anyhow::Error::from)?;
    let matches = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == tag || t.trim() == "*"));
    if matches {
        return Ok((StatusCode::NOT_MODIFIED, [(header::ETAG, tag_value)]).into_response());
    }
    Ok(([(header::ETAG, tag_value)], Json(graph)).into_response())
}

/// Updates name, description or goal.
#[utoipa::path(patch, path = "/graphs/{gid}", tag = "graphs", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id")), request_body = UpdateGraph,
    responses((status = 200, body = Graph), (status = 404, body = Problem), (status = 422, body = Problem)))]
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(gid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<UpdateGraph>,
) -> Result<Json<Graph>, AppError> {
    let name = req.name.as_deref().map(str::trim);
    repo::graphs::update(
        &state.db,
        auth.id,
        gid,
        name,
        req.description.as_deref(),
        req.goal.as_deref(),
    )
    .await
    .or_not_found("graph")?;
    if let Some(graph) = repo::graphs::summary(&state.db, gid).await? {
        state.hub.broadcast(gid, WsMessage::GraphUpdated { graph });
    }
    Ok(Json(editor::load_graph(&state, auth.id, gid).await?))
}

/// Deletes a graph with everything in it.
#[utoipa::path(delete, path = "/graphs/{gid}", tag = "graphs", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id")),
    responses((status = 204, description = "Deleted"), (status = 404, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(gid): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    if repo::graphs::delete(&state.db, auth.id, gid).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(AppError::NotFound("graph"))
    }
}

/// Dependency suggestions (similarity + BM25, near-duplicates skipped).
#[utoipa::path(get, path = "/graphs/{gid}/suggestions", tag = "graphs", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id")),
    responses((status = 200, body = [EdgeSuggestion]), (status = 404, body = Problem)))]
pub async fn suggestions(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(gid): Path<Uuid>,
) -> Result<Json<Vec<EdgeSuggestion>>, AppError> {
    let graph = editor::load_graph(&state, auth.id, gid).await?;
    Ok(Json(deps::suggestions(&graph.nodes, &graph.edges)))
}

/// Topological order, parallel levels, critical path, cycles and components.
#[utoipa::path(get, path = "/graphs/{gid}/analysis", tag = "graphs", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id")),
    responses((status = 200, body = GraphAnalysis), (status = 404, body = Problem)))]
pub async fn analysis(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(gid): Path<Uuid>,
) -> Result<Json<GraphAnalysis>, AppError> {
    let graph = editor::load_graph(&state, auth.id, gid).await?;
    Ok(Json(analysis::analyze(&graph.nodes, &graph.edges)))
}
