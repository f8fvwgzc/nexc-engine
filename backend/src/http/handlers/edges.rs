//! Edge create / delete.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::graph::{EdgeKind, GraphEdge};
use crate::domain::validation::{FieldErrors, Validate};
use crate::engine::editor;
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;

/// `POST /graphs/{gid}/edges` body. `depends_on`: source finishes before target.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateEdge {
    pub source: Uuid,
    pub target: Uuid,
    pub kind: Option<EdgeKind>,
}

impl Validate for CreateEdge {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.source == self.target {
            errors.add("target", "an edge cannot connect a node to itself");
        }
    }
}

/// Adds an edge; a `depends_on` edge that would create a cycle is a 409.
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
    let kind = req.kind.unwrap_or(EdgeKind::DependsOn);
    let edge = editor::create_edge(&state, auth.id, gid, req.source, req.target, kind).await?;
    Ok((StatusCode::CREATED, Json(edge)))
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
