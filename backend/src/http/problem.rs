//! RFC 7807 `application/problem+json` rendering of [`AppError`].

use std::collections::BTreeMap;

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use utoipa::ToSchema;

use crate::domain::AppError;

/// Media type of error responses.
pub const PROBLEM_JSON: &str = "application/problem+json";

/// An RFC 7807 problem document.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Problem {
    /// Always `about:blank`.
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Field validation messages (422 only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<BTreeMap<String, Vec<String>>>,
}

impl Problem {
    /// A problem for `status` with the standard reason phrase as title.
    pub fn new(status: StatusCode, detail: Option<String>) -> Self {
        Problem {
            kind: "about:blank".into(),
            title: status.canonical_reason().unwrap_or("Error").into(),
            status: status.as_u16(),
            detail,
            errors: None,
        }
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let body = serde_json::to_vec(&self).unwrap_or_default();
        let mut resp = (status, body).into_response();
        resp.headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(PROBLEM_JSON));
        resp
    }
}

impl AppError {
    /// HTTP status of this error.
    pub fn status(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::Validation(_) | AppError::Unprocessable(_) => {
                StatusCode::UNPROCESSABLE_ENTITY
            }
            AppError::RateLimited { .. } => StatusCode::TOO_MANY_REQUESTS,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        let mut problem = Problem::new(status, None);
        match &self {
            AppError::Validation(errors) => {
                problem.title = "Validation failed".into();
                problem.detail = Some("One or more fields are invalid.".into());
                problem.errors = Some(errors.as_map().clone());
            }
            AppError::Internal(err) => {
                tracing::error!(error = ?err, "internal error");
                problem.detail = Some("An unexpected error occurred.".into());
            }
            other => problem.detail = Some(other.to_string()),
        }
        let mut resp = problem.into_response();
        if let AppError::RateLimited { retry_after_secs } = self {
            resp.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from(retry_after_secs.max(1)),
            );
        }
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn renders_problem_json() {
        let resp = AppError::field("email", "invalid email").into_response();
        assert_eq!(resp.status(), 422);
        assert_eq!(resp.headers()[header::CONTENT_TYPE], PROBLEM_JSON);
        let json = body_json(resp).await;
        assert_eq!(json["type"], "about:blank");
        assert_eq!(json["title"], "Validation failed");
        assert_eq!(json["errors"]["email"][0], "invalid email");

        let resp = AppError::RateLimited {
            retry_after_secs: 7,
        }
        .into_response();
        assert_eq!(resp.headers()[header::RETRY_AFTER], "7");

        let json =
            body_json(AppError::Internal(anyhow::anyhow!("db password=xyz")).into_response()).await;
        assert!(
            !json.to_string().contains("xyz"),
            "internal details never leak"
        );
    }
}
