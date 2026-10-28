//! The skill library: the working knowledge agents are given, each skill
//! with the public sources it was written from (`docs/AGENT-CATALOG.md`).

use axum::Json;
use serde::Deserialize;
use utoipa::IntoParams;

use crate::domain::AppError;
use crate::domain::skill::{self, Skill, SkillSummary};
use crate::http::extract::{AuthUser, Path, Query};
use crate::http::problem::Problem;

/// Query of `GET /skills`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct SkillQuery {
    /// Only the skills of this category (`finance`, `shipping`, ...).
    pub category: Option<String>,
}

/// Every skill of the library, in id order, without their bodies.
#[utoipa::path(get, path = "/skills", tag = "agents", security(("bearer" = [])), params(SkillQuery),
    responses((status = 200, body = [SkillSummary]), (status = 401, body = Problem)))]
pub async fn list(_auth: AuthUser, Query(query): Query<SkillQuery>) -> Json<Vec<SkillSummary>> {
    let category = query.category.map(|c| c.trim().to_lowercase());
    Json(
        skill::library()
            .iter()
            .filter(|s| category.as_ref().is_none_or(|c| s.category == *c))
            .map(Skill::summary)
            .collect(),
    )
}

/// One skill with its working knowledge.
#[utoipa::path(get, path = "/skills/{id}", tag = "agents", security(("bearer" = [])),
    params(("id" = String, Path, description = "Skill id, e.g. fin-equity-valuation")),
    responses((status = 200, body = Skill), (status = 401, body = Problem), (status = 404, body = Problem)))]
pub async fn get(_auth: AuthUser, Path(id): Path<String>) -> Result<Json<Skill>, AppError> {
    skill::find(&id.to_lowercase())
        .cloned()
        .map(Json)
        .ok_or(AppError::NotFound("skill"))
}
