//! Request extractors whose rejections are problem+json [`AppError`]s.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{ConnectInfo, FromRequest, FromRequestParts, Request};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::user::Role;
use crate::domain::validation::{FieldErrors, Validate};

/// The authenticated caller (from a valid bearer access token).
#[derive(Debug, Clone, Copy)]
pub struct AuthUser {
    pub id: Uuid,
    pub role: Role,
}

/// Extracts the bearer token from request headers, if any.
pub fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|t| !t.is_empty())
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token =
            bearer_token(&parts.headers).ok_or(AppError::Unauthorized("missing bearer token"))?;
        let claims = state
            .jwt
            .verify(token)
            .ok_or(AppError::Unauthorized("invalid or expired token"))?;
        state.limiters.check_user(claims.sub)?;
        Ok(AuthUser {
            id: claims.sub,
            role: claims.role,
        })
    }
}

/// The client address: the TCP peer, or the last `X-Forwarded-For` hop
/// when `NEXC_TRUST_PROXY=true` (the address our own proxy appended).
#[derive(Debug, Clone, Copy)]
pub struct ClientIp(pub IpAddr);

/// Resolves the client IP of a request.
pub fn client_ip(parts: &Parts, trust_proxy: bool) -> IpAddr {
    if trust_proxy
        && let Some(ip) = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.rsplit(',').next())
            .and_then(|ip| ip.trim().parse().ok())
    {
        return ip;
    }
    parts
        .extensions
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip())
        .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED))
}

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        Ok(ClientIp(client_ip(parts, state.settings.trust_proxy)))
    }
}

/// JSON body that is deserialised strictly and then validated.
#[derive(Debug, Clone)]
pub struct ValidatedJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let axum::Json(value) = axum::Json::<T>::from_request(req, state).await?;
        let mut errors = FieldErrors::default();
        value.validate(&mut errors);
        errors.into_result()?;
        Ok(ValidatedJson(value))
    }
}

impl From<JsonRejection> for AppError {
    fn from(rejection: JsonRejection) -> Self {
        match rejection {
            JsonRejection::JsonDataError(e) => AppError::Unprocessable(e.body_text()),
            JsonRejection::JsonSyntaxError(_) => AppError::BadRequest("malformed JSON body".into()),
            JsonRejection::MissingJsonContentType(_) => {
                AppError::BadRequest("expected Content-Type: application/json".into())
            }
            other if other.status() == StatusCode::PAYLOAD_TOO_LARGE => AppError::PayloadTooLarge,
            other => AppError::BadRequest(other.body_text()),
        }
    }
}

/// Path parameters (invalid ids are a 400).
#[derive(Debug, FromRequestParts)]
#[from_request(via(axum::extract::Path), rejection(AppError))]
pub struct Path<T>(pub T);

impl From<PathRejection> for AppError {
    fn from(rejection: PathRejection) -> Self {
        AppError::BadRequest(format!("invalid path parameter: {}", rejection.body_text()))
    }
}

/// Query string parameters.
#[derive(Debug, FromRequestParts)]
#[from_request(via(axum::extract::Query), rejection(AppError))]
pub struct Query<T>(pub T);

impl From<QueryRejection> for AppError {
    fn from(rejection: QueryRejection) -> Self {
        AppError::BadRequest(format!("invalid query string: {}", rejection.body_text()))
    }
}
