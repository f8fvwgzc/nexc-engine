//! Edge create / update / delete.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::graph::GraphEdge;
use crate::domain::ontology::REASON_MAX;
use crate::domain::validation::{FieldErrors, Validate, check_max_len};
use crate::engine::editor;
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;

/// `POST /graphs/{gid}/edges` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateEdge {
    pub source: Uuid,
    pub target: Uuid,
    /// Key of a relation type of the graph's ontology (its dependency
    /// relation when absent).
    pub kind: Option<String>,
    /// Why the two nodes are related this way.
    #[serde(default)]
    pub reason: String,
}

/// `PATCH /graphs/{gid}/edges/{eid}` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateEdge {
    pub reason: String,
}

impl Validate for UpdateEdge {
    fn validate(&self, errors: &mut FieldErrors) {
        check_max_len(errors, "reason", &self.reason, REASON_MAX);
    }
}

impl Validate for CreateEdge {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.source == self.target {
            errors.add("target", "an edge cannot connect a node to itself");
        }
        check_max_len(errors, "reason", &self.reason, REASON_MAX);
    }
}

/// Adds an edge; an edge of a blocking relation that would create a cycle is a 409.
#[utoipa::path(post, path = "/graphs/{gid}/edges", tag = "edges", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id")), request_body = CreateEdge,
    responses(
        (status = 201, body = GraphEdge),
        (status = 404, body = Problem),
        (status = 409, description = "Would create a cycle, or already exists", body = Problem),
        (status = 422, body = Problem),
    ))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(gid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<CreateEdge>,
) -> Result<(StatusCode, Json<GraphEdge>), AppError> {
    let edge = editor::create_edge(
        &state,
        auth.id,
        gid,
        req.source,
        req.target,
        req.kind,
        req.reason.trim(),
    )
    .await?;
    Ok((StatusCode::CREATED, Json(edge)))
}

/// Rewrites why an edge exists.
#[utoipa::path(patch, path = "/graphs/{gid}/edges/{eid}", tag = "edges", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id"), ("eid" = Uuid, Path, description = "Edge id")),
    request_body = UpdateEdge,
    responses((status = 200, body = GraphEdge), (status = 404, body = Problem), (status = 422, body = Problem)))]
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((gid, eid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<UpdateEdge>,
) -> Result<Json<GraphEdge>, AppError> {
    Ok(Json(
        editor::update_edge_reason(&state, auth.id, gid, eid, req.reason.trim()).await?,
    ))
}

/// Deletes an edge.
#[utoipa::path(delete, path = "/graphs/{gid}/edges/{eid}", tag = "edges", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id"), ("eid" = Uuid, Path, description = "Edge id")),
    responses((status = 204, description = "Deleted"), (status = 404, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((gid, eid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    editor::delete_edge(&state, auth.id, gid, eid).await?;
    Ok(StatusCode::NO_CONTENT)
}
