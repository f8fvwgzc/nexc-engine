//! Node create / update / delete.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::{Deserialize, Deserializer};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::agent::is_valid_role;
use crate::domain::graph::{
    CONTENT_MAX_BYTES, Executor, GraphNode, NodePatch, NodeStatus, TAG_LEN_MAX, TAGS_MAX,
    TITLE_MAX, normalize_tags,
};
use crate::domain::validation::{FieldErrors, Validate, check_coordinate, check_text};
use crate::engine::editor;
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;

/// Distinguishes an absent field (`None`) from an explicit `null` (`Some(None)`).
fn double_option<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<Option<T>>, D::Error> {
    Option::<T>::deserialize(d).map(Some)
}

fn check_content(errors: &mut FieldErrors, content: &str) {
    if content.len() > CONTENT_MAX_BYTES {
        errors.add(
            "content",
            format!("must be at most {} KiB", CONTENT_MAX_BYTES / 1024),
        );
    }
}

fn check_tags(errors: &mut FieldErrors, tags: &[String]) {
    if tags.len() > TAGS_MAX || tags.iter().any(|t| t.chars().count() > TAG_LEN_MAX) {
        errors.add(
            "tags",
            format!("at most {TAGS_MAX} tags of at most {TAG_LEN_MAX} characters"),
        );
    }
}

fn check_role(errors: &mut FieldErrors, role: &str) {
    if !role.is_empty() && !is_valid_role(role) {
        errors.add(
            "agent_role",
            "must be lowercase letters, digits, `_` or `-` (max 64)",
        );
    }
}

/// `POST /graphs/{gid}/nodes` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateNode {
    pub title: String,
    #[serde(default)]
    pub content: String,
    /// Key of a node type of the graph's ontology (its default type when absent).
    pub kind: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub executor: Option<Executor>,
    pub agent_role: Option<String>,
}

impl Validate for CreateNode {
    fn validate(&self, errors: &mut FieldErrors) {
        check_text(errors, "title", &self.title, TITLE_MAX);
        check_content(errors, &self.content);
        check_tags(errors, &self.tags);
        check_coordinate(errors, "x", self.x.unwrap_or_default());
        check_coordinate(errors, "y", self.y.unwrap_or_default());
        check_role(errors, self.agent_role.as_deref().unwrap_or_default());
    }
}

/// `PATCH /graphs/{gid}/nodes/{nid}` body: any subset of the mutable node
/// fields. `agent_role` and `output` accept `null` to clear them.
#[derive(Debug, Default, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateNode {
    pub title: Option<String>,
    pub content: Option<String>,
    /// Key of a node type of the graph's ontology.
    pub kind: Option<String>,
    pub tags: Option<Vec<String>>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub status: Option<NodeStatus>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>, nullable)]
    pub agent_role: Option<Option<String>>,
    pub executor: Option<Executor>,
    #[serde(default, deserialize_with = "double_option")]
    #[schema(value_type = Option<String>, nullable)]
    pub output: Option<Option<String>>,
}

impl Validate for UpdateNode {
    fn validate(&self, errors: &mut FieldErrors) {
        if let Some(t) = &self.title {
            check_text(errors, "title", t, TITLE_MAX);
        }
        if let Some(c) = &self.content {
            check_content(errors, c);
        }
        if let Some(tags) = &self.tags {
            check_tags(errors, tags);
        }
        for (field, v) in [("x", self.x), ("y", self.y)] {
            if let Some(v) = v {
                check_coordinate(errors, field, v);
            }
        }
        if let Some(Some(role)) = &self.agent_role {
            check_role(errors, role);
        }
    }
}

impl From<UpdateNode> for NodePatch {
    fn from(u: UpdateNode) -> Self {
        NodePatch {
            title: u.title.map(|t| t.trim().to_owned()),
            content: u.content,
            kind: u.kind,
            tags: u.tags.map(|t| normalize_tags(&t)),
            x: u.x,
            y: u.y,
            status: u.status,
            agent_role: u
                .agent_role
                .map(|r| r.map(|r| r.trim().to_owned()).filter(|r| !r.is_empty())),
            executor: u.executor,
            output: u.output,
        }
    }
}

/// Adds a node.
#[utoipa::path(post, path = "/graphs/{gid}/nodes", tag = "nodes", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id")), request_body = CreateNode,
    responses((status = 201, body = GraphNode), (status = 404, body = Problem), (status = 422, body = Problem)))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(gid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<CreateNode>,
) -> Result<(StatusCode, Json<GraphNode>), AppError> {
    let new = editor::NewNode {
        title: req.title.trim().to_owned(),
        content: req.content,
        kind: req.kind,
        tags: normalize_tags(&req.tags),
        x: req.x.unwrap_or_default(),
        y: req.y.unwrap_or_default(),
        agent_role: req
            .agent_role
            .map(|r| r.trim().to_owned())
            .filter(|r| !r.is_empty()),
        executor: req.executor,
    };
    Ok((
        StatusCode::CREATED,
        Json(editor::create_node(&state, auth.id, gid, new).await?),
    ))
}

/// Updates a node.
#[utoipa::path(patch, path = "/graphs/{gid}/nodes/{nid}", tag = "nodes", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id"), ("nid" = Uuid, Path, description = "Node id")),
    request_body = UpdateNode,
    responses((status = 200, body = GraphNode), (status = 404, body = Problem), (status = 422, body = Problem)))]
pub async fn update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((gid, nid)): Path<(Uuid, Uuid)>,
    ValidatedJson(req): ValidatedJson<UpdateNode>,
) -> Result<Json<GraphNode>, AppError> {
    Ok(Json(
        editor::update_node(&state, auth.id, gid, nid, req.into()).await?,
    ))
}

/// Deletes a node and its edges.
#[utoipa::path(delete, path = "/graphs/{gid}/nodes/{nid}", tag = "nodes", security(("bearer" = [])),
    params(("gid" = Uuid, Path, description = "Graph id"), ("nid" = Uuid, Path, description = "Node id")),
    responses((status = 204, description = "Deleted"), (status = 404, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((gid, nid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    editor::delete_node(&state, auth.id, gid, nid).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_distinguishes_null_from_absent() {
        let absent: UpdateNode = serde_json::from_str("{}").unwrap();
        assert!(absent.agent_role.is_none());
        let cleared: UpdateNode = serde_json::from_str(r#"{"agent_role": null}"#).unwrap();
        assert_eq!(cleared.agent_role, Some(None));
        assert!(
            serde_json::from_str::<UpdateNode>(r#"{"id": "x"}"#).is_err(),
            "unknown fields are rejected"
        );
    }
}
