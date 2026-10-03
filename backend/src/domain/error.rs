//! The application error type. The HTTP layer renders it as RFC 7807
//! `application/problem+json` (see `http::problem`).

use super::validation::FieldErrors;

/// Every failure a request can end with.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// 400 – malformed input that is not a field validation problem.
    #[error("{0}")]
    BadRequest(String),
    /// 401 – missing or invalid credentials. The message is always generic.
    #[error("{0}")]
    Unauthorized(&'static str),
    /// 403 – authenticated but not allowed.
    #[error("{0}")]
    Forbidden(String),
    /// 404 – unknown resource (also used for resources owned by someone else).
    #[error("{0} not found")]
    NotFound(&'static str),
    /// 409 – the request conflicts with current state (cycles, duplicates).
    #[error("{0}")]
    Conflict(String),
    /// 413 – request body too large.
    #[error("request body too large")]
    PayloadTooLarge,
    /// 422 – field validation failed.
    #[error("validation failed")]
    Validation(FieldErrors),
    /// 422 – semantically invalid request (e.g. no LLM key configured).
    #[error("{0}")]
    Unprocessable(String),
    /// 429 – rate limited; retry after the given number of seconds.
    #[error("too many requests")]
    RateLimited { retry_after_secs: u64 },
    /// 500 – unexpected failure; details are logged, never returned.
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl AppError {
    /// Shorthand for a single-field validation error.
    pub fn field(field: &str, message: impl Into<String>) -> Self {
        let mut errors = FieldErrors::default();
        errors.add(field, message);
        AppError::Validation(errors)
    }
}
