//! `Idempotency-Key` (CONTRACT §3): a client that may retry a write (its
//! connection broke, the replica it reached went away) sends a key with it.
//! The first request with a key is handled and its response kept; a retry
//! with the same key and the same body gets that response again, marked
//! `Idempotent-Replayed: true`, instead of a second run, issue, node or
//! decision. The same key with another body is refused (422); a retry while
//! the first is still being handled is told to wait (409, `Retry-After`).
//! Keys belong to the account that sent them and to the method and path
//! they were sent to, and are kept 24 hours (a housekeeping job deletes
//! them). Server errors and rate limits are not kept: retrying them runs
//! the request again.

use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{MatchedPath, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::app::AppState;
use crate::app::router::BODY_LIMIT;
use crate::domain::AppError;
use crate::http::extract::bearer_token;
use crate::http::problem::Problem;
use crate::repo::idempotency::{self, Claim};

/// The request header.
pub const HEADER: HeaderName = HeaderName::from_static("idempotency-key");
/// The response header of a replayed response.
pub const REPLAYED: HeaderName = HeaderName::from_static("idempotent-replayed");
/// Longest key accepted.
pub const KEY_MAX: usize = 255;
/// Largest response kept for replay; a bigger one is not kept.
const RESPONSE_MAX: usize = 1024 * 1024;
/// How long a first request may be under way before its claim is taken to
/// have died with the instance handling it, and the key is free again.
pub const STALE_CLAIM: Duration = Duration::from_secs(60);

/// The writes that honour the header (method, route as matched).
const ROUTES: [&str; 5] = [
    "/api/v1/graphs/{gid}/runs",
    "/api/v1/workspaces/{wid}/teams/{tid}/issues",
    "/api/v1/graphs/{gid}/nodes",
    "/api/v1/approvals/{id}/approve",
    "/api/v1/approvals/{id}/deny",
];

/// Whether `key` is a key: 1 to 255 visible ASCII characters.
pub fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= KEY_MAX && key.bytes().all(|b| b.is_ascii_graphic())
}

/// What identifies a request's content: its body as canonical JSON when it
/// is JSON (the same object however it was spaced or ordered), its bytes otherwise.
pub fn request_hash(body: &[u8]) -> [u8; 32] {
    let canonical = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| serde_json::to_vec(&sorted(value)).ok());
    Sha256::digest(canonical.as_deref().unwrap_or(body)).into()
}

/// `value` with the keys of every object in order.
fn sorted(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> =
                map.into_iter().map(|(k, v)| (k, sorted(v))).collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// The account a request acts as, when its access token is valid; a request
/// without one goes to its handler, which answers 401.
fn account(state: &AppState, headers: &HeaderMap) -> Option<Uuid> {
    let claims = state.jwt.verify(bearer_token(headers)?)?;
    state
        .sessions
        .admits(claims.sub, claims.epoch)
        .then_some(claims.sub)
}

/// Honours `Idempotency-Key` on the writes of [`ROUTES`]; every other
/// request passes untouched.
pub async fn replay(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let Some(raw) = req.headers().get(&HEADER) else {
        return next.run(req).await;
    };
    let honoured = req.method() == Method::POST
        && req
            .extensions()
            .get::<MatchedPath>()
            .is_some_and(|m| ROUTES.contains(&m.as_str()));
    if !honoured {
        return next.run(req).await;
    }
    let Some(key) = raw
        .to_str()
        .ok()
        .filter(|k| valid_key(k))
        .map(str::to_owned)
    else {
        return Problem::new(
            StatusCode::BAD_REQUEST,
            Some(format!(
                "Idempotency-Key must be 1 to {KEY_MAX} visible ASCII characters"
            )),
        )
        .into_response();
    };
    let Some(user) = account(&state, req.headers()) else {
        return next.run(req).await;
    };
    let route = format!("{} {}", req.method(), req.uri().path());
    let (parts, body) = req.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, BODY_LIMIT).await else {
        return AppError::PayloadTooLarge.into_response();
    };
    let hash = request_hash(&bytes);
    match idempotency::claim(&state.db, user, &route, &key, &hash, STALE_CLAIM).await {
        Ok(Claim::First) => {}
        Ok(Claim::Done(kept)) => {
            let mut resp = (
                StatusCode::from_u16(kept.status as u16).unwrap_or(StatusCode::OK),
                Bytes::from(kept.body),
            )
                .into_response();
            if let Some(kind) = kept
                .content_type
                .and_then(|t| HeaderValue::from_str(&t).ok())
            {
                resp.headers_mut().insert(header::CONTENT_TYPE, kind);
            }
            resp.headers_mut()
                .insert(REPLAYED, HeaderValue::from_static("true"));
            return resp;
        }
        Ok(Claim::OtherRequest) => {
            return AppError::Unprocessable(
                "this Idempotency-Key was used with a different request; use a new key".into(),
            )
            .into_response();
        }
        Ok(Claim::UnderWay) => {
            let mut resp = AppError::Conflict(
                "a request with this Idempotency-Key is still being handled; retry shortly".into(),
            )
            .into_response();
            resp.headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
            return resp;
        }
        Err(err) => return AppError::from(err).into_response(),
    }
    let resp = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;
    let status = resp.status();
    // Errors of the server and rate limits are not the request's answer: a retry runs it again.
    if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
        release(&state, user, &route, &key).await;
        return resp;
    }
    let (parts, body) = resp.into_parts();
    let bytes = match axum::body::to_bytes(body, RESPONSE_MAX).await {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::error!(error = %err, "response not kept for its Idempotency-Key");
            release(&state, user, &route, &key).await;
            return AppError::Internal(anyhow::anyhow!("response body unreadable")).into_response();
        }
    };
    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok());
    if let Err(err) = idempotency::complete(
        &state.db,
        user,
        &route,
        &key,
        status.as_u16() as i16,
        &bytes,
        content_type,
    )
    .await
    {
        // The write happened; only its replay is lost. A retry gets 409 until the claim goes stale.
        tracing::error!(error = %err, "response not kept for its Idempotency-Key");
    }
    Response::from_parts(parts, Body::from(bytes))
}

/// Frees a key whose request got no answer worth keeping.
async fn release(state: &AppState, user: Uuid, route: &str, key: &str) {
    if let Err(err) = idempotency::release(&state.db, user, route, key).await {
        tracing::error!(error = %err, "Idempotency-Key not released");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_visible_ascii_of_bounded_length() {
        assert!(valid_key("a") && valid_key(&"k".repeat(KEY_MAX)));
        assert!(valid_key("0199c3e4-retry_1:x"));
        assert!(!valid_key("") && !valid_key(&"k".repeat(KEY_MAX + 1)));
        assert!(!valid_key("with space") && !valid_key("naïve") && !valid_key("tab\t"));
    }

    #[test]
    fn the_same_json_hashes_the_same_however_it_is_written() {
        assert_eq!(
            request_hash(br#"{"a": 1, "b": [true]}"#),
            request_hash(br#"{"b":[true],"a":1}"#)
        );
        assert_ne!(request_hash(br#"{"a": 1}"#), request_hash(br#"{"a": 2}"#));
        assert_eq!(request_hash(b""), request_hash(b""));
        assert_ne!(request_hash(b"not json"), request_hash(b"not json!"));
    }
}
