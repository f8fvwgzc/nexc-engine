//! Built-in starter graphs.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use serde::Deserialize;
use utoipa::ToSchema;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::graph::{GRAPH_NAME_MAX, Graph};
use crate::domain::template::{self, GraphTemplate, TEMPLATE_TOPIC_MAX};
use crate::domain::validation::{FieldErrors, Validate, check_text};
use crate::engine::templates;
use crate::http::extract::{AuthUser, ValidatedJson};
use crate::http::problem::Problem;

/// `POST /graphs/from-template` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct FromTemplate {
    pub template_id: String,
    /// Graph name (default: the template's name, plus the topic when given).
    pub name: Option<String>,
    /// What this instance is about, e.g. the research question or product. It is put at the top
    /// of the graph goal so every planner and node prompt sees it.
    pub topic: Option<String>,
}

impl Validate for FromTemplate {
    fn validate(&self, errors: &mut FieldErrors) {
        check_text(errors, "template_id", &self.template_id, 100);
        if let Some(name) = &self.name {
            check_text(errors, "name", name, GRAPH_NAME_MAX);
        }
        if let Some(topic) = &self.topic {
            check_text(errors, "topic", topic, TEMPLATE_TOPIC_MAX);
        }
    }
}

/// The built-in templates.
#[utoipa::path(get, path = "/templates", tag = "templates", security(("bearer" = [])),
    responses((status = 200, body = [GraphTemplate]), (status = 401, body = Problem)))]
pub async fn list(_auth: AuthUser) -> Json<Vec<GraphTemplate>> {
    Json(template::builtin().iter().map(|t| t.summary()).collect())
}

/// Creates a new graph from a template.
#[utoipa::path(post, path = "/graphs/from-template", tag = "templates", security(("bearer" = [])),
    request_body = FromTemplate,
    responses((status = 201, body = Graph), (status = 404, description = "Unknown template", body = Problem)))]
pub async fn instantiate(
    State(state): State<AppState>,
    auth: AuthUser,
    ValidatedJson(req): ValidatedJson<FromTemplate>,
) -> Result<(StatusCode, Json<Graph>), AppError> {
    let graph =
        templates::instantiate(&state, auth.id, req.template_id.trim(), req.name, req.topic)
            .await?;
    Ok((StatusCode::CREATED, Json(graph)))
}
