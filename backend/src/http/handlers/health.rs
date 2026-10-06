//! Liveness, readiness and Prometheus metrics.

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::user::Role;
use crate::http::extract::bearer_token;
use crate::http::problem::Problem;
use crate::security::random::constant_time_eq;

/// Health probe response.
#[derive(Debug, Serialize, ToSchema)]
pub struct Health {
    pub status: &'static str,
    pub version: &'static str,
}

/// Liveness: the process serves HTTP.
#[utoipa::path(get, path = "/healthz", tag = "health", responses((status = 200, body = Health)))]
pub async fn healthz() -> Json<Health> {
    Json(Health {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

/// Readiness: the database answers.
#[utoipa::path(get, path = "/readyz", tag = "health",
    responses((status = 200, body = Health), (status = 503, description = "Database unavailable", body = Problem)))]
pub async fn readyz(State(state): State<AppState>) -> Response {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => Json(Health {
            status: "ok",
            version: env!("CARGO_PKG_VERSION"),
        })
        .into_response(),
        Err(_) => Problem::new(
            StatusCode::SERVICE_UNAVAILABLE,
            Some("database unavailable".into()),
        )
        .into_response(),
    }
}

/// Prometheus metrics; requires `NEXC_METRICS_TOKEN` or an admin access token.
#[utoipa::path(get, path = "/metrics", tag = "health", security(("bearer" = [])),
    responses(
        (status = 200, description = "Prometheus text exposition", content_type = "text/plain", body = String),
        (status = 401, body = Problem),
        (status = 403, body = Problem),
    ))]
pub async fn metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let token = bearer_token(&headers).ok_or(AppError::Unauthorized("missing bearer token"))?;
    let by_token = state
        .settings
        .metrics_token
        .as_ref()
        .is_some_and(|expected| constant_time_eq(expected.expose().as_bytes(), token.as_bytes()));
    if !by_token {
        let claims = state
            .jwt
            .verify(token)
            .ok_or(AppError::Unauthorized("invalid token"))?;
        if !state.sessions.admits(claims.sub, claims.epoch) {
            return Err(AppError::Unauthorized("invalid token"));
        }
        if claims.role != Role::Admin {
            return Err(AppError::Forbidden("metrics require an admin".into()));
        }
    }
    let content_type = HeaderValue::from_static("text/plain; version=0.0.4; charset=utf-8");
    Ok((
        [(header::CONTENT_TYPE, content_type)],
        state.metrics.render(),
    )
        .into_response())
}
