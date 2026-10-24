//! Conditional reads (CONTRACT §3): every JSON read carries a strong `ETag`
//! over its body and `Cache-Control: private, no-cache`, so a client that
//! keeps the body may ask again with `If-None-Match` and get a `304` with no
//! body when nothing changed. Streams (SSE, downloads) and errors are left
//! alone; a handler that computes its own tag keeps it, and one that serves a
//! public document (a key set, an agent card) keeps its `public` policy, so
//! the edge and any other shared cache may keep it too.

use axum::body::{Body, HttpBody as _};
use axum::extract::Request;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use http_body_util::BodyExt;
use sha2::{Digest, Sha256};

use crate::http::problem::Problem;

/// Largest body that is tagged; a bigger one streams through as it is.
pub const ETAG_MAX_BYTES: u64 = 8 * 1024 * 1024;
/// What every tagged read says about caching: a client may keep the body
/// but must ask (with `If-None-Match`) before reusing it; a shared cache
/// may not keep it at all.
pub const CACHE_CONTROL: &str = "private, no-cache";

/// The policy a tagged read goes out with: the handler's own when it made
/// the answer public, [`CACHE_CONTROL`] otherwise.
fn policy(headers: &HeaderMap) -> HeaderValue {
    headers
        .get(header::CACHE_CONTROL)
        .filter(|v| v.to_str().is_ok_and(|v| v.starts_with("public")))
        .cloned()
        .unwrap_or(HeaderValue::from_static(CACHE_CONTROL))
}

/// Strong entity tag of a body: the first 128 bits of its SHA-256, quoted.
pub fn etag_of(body: &[u8]) -> String {
    let digest = Sha256::digest(body);
    format!("\"{}\"", hex::encode(&digest[..16]))
}

/// Whether an `If-None-Match` value names `etag`: the tag itself, its weak
/// form, or `*`, in a comma-separated list.
pub fn matches(if_none_match: &str, etag: &str) -> bool {
    if_none_match.split(',').any(|candidate| {
        let candidate = candidate.trim();
        candidate == "*" || candidate == etag || candidate.strip_prefix("W/") == Some(etag)
    })
}

fn is_json(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("application/json"))
}

/// The probes: a liveness or readiness answer is never kept.
fn is_probe(path: &str) -> bool {
    path.ends_with("/healthz") || path.ends_with("/readyz")
}

/// Tags JSON reads and answers `If-None-Match`.
pub async fn conditional_reads(request: Request, next: Next) -> Response {
    let is_get = request.method() == Method::GET && !is_probe(request.uri().path());
    let if_none_match = request
        .headers()
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let mut response = next.run(request).await;
    if !is_get {
        return response;
    }
    // A handler that tagged the answer itself (and answered the condition) only needs the policy.
    if response.headers().contains_key(header::ETAG) {
        let policy = policy(response.headers());
        response.headers_mut().insert(header::CACHE_CONTROL, policy);
        return response;
    }
    if response.status() != StatusCode::OK || !is_json(&response) {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    if body.size_hint().upper().is_none_or(|n| n > ETAG_MAX_BYTES) {
        return Response::from_parts(parts, body);
    }
    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(err) => {
            tracing::error!(error = %err, "a response body could not be read for its ETag");
            return Problem::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                Some("the response could not be read".into()),
            )
            .into_response();
        }
    };
    let tag = etag_of(&bytes);
    let tag_value = HeaderValue::from_str(&tag).expect("an ETag is ASCII");
    parts.headers.insert(header::ETAG, tag_value);
    let policy = policy(&parts.headers);
    parts.headers.insert(header::CACHE_CONTROL, policy);
    if if_none_match.is_some_and(|value| matches(&value, &tag)) {
        parts.status = StatusCode::NOT_MODIFIED;
        parts.headers.remove(header::CONTENT_TYPE);
        parts.headers.remove(header::CONTENT_LENGTH);
        return Response::from_parts(parts, Body::empty());
    }
    Response::from_parts(parts, Body::from(bytes))
}

#[cfg(test)]
mod tests {
    use axum::Router;
    use axum::middleware::from_fn;
    use axum::routing::{get, post};
    use serde_json::json;
    use tower::ServiceExt as _;

    use super::*;

    #[test]
    fn tags_are_strong_quoted_and_change_with_the_body() {
        let a = etag_of(br#"{"id":1}"#);
        let b = etag_of(br#"{"id":2}"#);
        assert!(a.starts_with('"') && a.ends_with('"') && a.len() == 34);
        assert_ne!(a, b);
        assert_eq!(a, etag_of(br#"{"id":1}"#), "deterministic");
    }

    #[test]
    fn if_none_match_lists_weak_forms_and_stars_match() {
        let tag = etag_of(b"x");
        assert!(matches(&tag, &tag));
        assert!(matches(&format!("\"other\", {tag}"), &tag));
        assert!(matches(&format!("W/{tag}"), &tag));
        assert!(matches("*", &tag));
        assert!(!matches("\"other\"", &tag));
        assert!(!matches("", &tag));
    }

    fn app() -> Router {
        Router::new()
            .route("/thing", get(|| async { axum::Json(json!({"id": 7})) }))
            .route("/thing", post(|| async { axum::Json(json!({"id": 7})) }))
            .route(
                "/text",
                get(|| async { ([(header::CONTENT_TYPE, "text/plain")], "plain") }),
            )
            .route(
                "/tagged",
                get(|| async { ([(header::ETAG, "\"mine\"")], axum::Json(json!({"id": 1}))) }),
            )
            .route(
                "/keys",
                get(|| async {
                    (
                        [(header::CACHE_CONTROL, "public, max-age=300")],
                        axum::Json(json!({"keys": []})),
                    )
                }),
            )
            .route(
                "/private",
                get(|| async {
                    (
                        [(header::CACHE_CONTROL, "no-store")],
                        axum::Json(json!({"id": 2})),
                    )
                }),
            )
            .route(
                "/missing",
                get(|| async { (StatusCode::NOT_FOUND, axum::Json(json!({"status": 404}))) }),
            )
            .route(
                "/healthz",
                get(|| async { axum::Json(json!({"status": "ok"})) }),
            )
            .layer(from_fn(conditional_reads))
    }

    async fn get_with(path: &str, if_none_match: Option<&str>, method: Method) -> Response {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(value) = if_none_match {
            request = request.header(header::IF_NONE_MATCH, value);
        }
        app()
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn body_of(response: Response) -> Vec<u8> {
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec()
    }

    #[tokio::test]
    async fn json_reads_are_tagged_and_answer_the_condition() {
        let first = get_with("/thing", None, Method::GET).await;
        assert_eq!(first.status(), StatusCode::OK);
        let tag = first.headers()[header::ETAG].to_str().unwrap().to_owned();
        assert_eq!(first.headers()[header::CACHE_CONTROL], CACHE_CONTROL);
        assert_eq!(body_of(first).await, br#"{"id":7}"#);

        let cached = get_with("/thing", Some(&tag), Method::GET).await;
        assert_eq!(cached.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(cached.headers()[header::ETAG], tag.as_str());
        assert_eq!(cached.headers()[header::CACHE_CONTROL], CACHE_CONTROL);
        assert!(cached.headers().get(header::CONTENT_TYPE).is_none());
        assert!(body_of(cached).await.is_empty(), "a 304 carries no body");

        let other = get_with("/thing", Some("\"stale\""), Method::GET).await;
        assert_eq!(other.status(), StatusCode::OK);
        assert_eq!(body_of(other).await, br#"{"id":7}"#);
    }

    #[tokio::test]
    async fn a_public_document_keeps_its_policy_and_is_tagged() {
        let first = get_with("/keys", None, Method::GET).await;
        assert_eq!(
            first.headers()[header::CACHE_CONTROL],
            "public, max-age=300"
        );
        let tag = first.headers()[header::ETAG].to_str().unwrap().to_owned();
        let cached = get_with("/keys", Some(&tag), Method::GET).await;
        assert_eq!(cached.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            cached.headers()[header::CACHE_CONTROL],
            "public, max-age=300"
        );

        let private = get_with("/private", None, Method::GET).await;
        assert_eq!(
            private.headers()[header::CACHE_CONTROL],
            CACHE_CONTROL,
            "only a public policy is the handler's to choose: a JSON read is always kept privately"
        );
    }

    #[tokio::test]
    async fn writes_text_errors_and_handler_tags_are_left_alone() {
        let written = get_with("/thing", Some("*"), Method::POST).await;
        assert_eq!(written.status(), StatusCode::OK);
        assert!(written.headers().get(header::ETAG).is_none());

        let text = get_with("/text", Some("*"), Method::GET).await;
        assert_eq!(text.status(), StatusCode::OK);
        assert!(text.headers().get(header::ETAG).is_none());
        assert!(text.headers().get(header::CACHE_CONTROL).is_none());

        let missing = get_with("/missing", Some("*"), Method::GET).await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert!(missing.headers().get(header::ETAG).is_none());

        let tagged = get_with("/tagged", None, Method::GET).await;
        assert_eq!(tagged.headers()[header::ETAG], "\"mine\"");
        assert_eq!(tagged.headers()[header::CACHE_CONTROL], CACHE_CONTROL);

        let probe = get_with("/healthz", Some("*"), Method::GET).await;
        assert_eq!(probe.status(), StatusCode::OK);
        assert!(
            probe.headers().get(header::ETAG).is_none(),
            "a probe is never kept"
        );
    }
}
