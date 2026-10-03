//! HTTP middleware: security headers, problem+json for framework errors,
//! request metrics and rate limiting.

pub mod rate_limit;

use std::time::Instant;

use axum::extract::{MatchedPath, Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::problem::{PROBLEM_JSON, Problem};
use crate::app::AppState;

/// CSP for JSON / SSE API responses: nothing may load.
const API_CSP: &str =
    "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";
/// CSP for the Scalar API reference page (`/api/docs`).
const DOCS_CSP: &str = "default-src 'none'; script-src https://cdn.jsdelivr.net; \
    style-src 'self' 'unsafe-inline' https://cdn.jsdelivr.net https://fonts.googleapis.com; \
    font-src 'self' data: https://fonts.gstatic.com https://fonts.scalar.com https://cdn.jsdelivr.net; \
    img-src 'self' data: https:; connect-src 'self'; worker-src blob:; \
    frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

fn set(headers: &mut HeaderMap, name: HeaderName, value: &'static str) {
    headers
        .entry(name)
        .or_insert(HeaderValue::from_static(value));
}

/// Adds security headers to every response and `Cache-Control: no-store`
/// to API responses.
pub async fn security_headers(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let is_docs = req.uri().path().starts_with("/api/docs");
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    set(
        h,
        header::CONTENT_SECURITY_POLICY,
        if is_docs { DOCS_CSP } else { API_CSP },
    );
    set(h, header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    set(h, header::X_FRAME_OPTIONS, "DENY");
    set(h, header::REFERRER_POLICY, "no-referrer");
    set(
        h,
        HeaderName::from_static("permissions-policy"),
        "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
    );
    set(
        h,
        HeaderName::from_static("cross-origin-opener-policy"),
        "same-origin",
    );
    set(
        h,
        HeaderName::from_static("cross-origin-resource-policy"),
        "same-site",
    );
    set(h, header::CACHE_CONTROL, "no-store");
    if state.settings.is_production() {
        set(
            h,
            header::STRICT_TRANSPORT_SECURITY,
            "max-age=63072000; includeSubDomains",
        );
    }
    resp
}

/// Rewrites framework-generated error responses (unknown route, wrong
/// method, oversized body, timeout, …) as problem+json.
pub async fn problem_errors(req: Request, next: Next) -> Response {
    let resp = next.run(req).await;
    let status = resp.status();
    let is_json = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with(PROBLEM_JSON) || ct.starts_with("application/json"));
    if status.is_client_error() || status.is_server_error() {
        if is_json {
            return resp;
        }
        let mut problem = Problem::new(status, None).into_response();
        for name in [header::ALLOW, header::RETRY_AFTER] {
            if let Some(v) = resp.headers().get(&name) {
                problem.headers_mut().insert(name, v.clone());
            }
        }
        return problem;
    }
    resp
}

/// Counts requests by method, route template and status, and records latency.
pub async fn track_metrics(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = req.method().as_str().to_owned();
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|p| p.as_str().to_owned())
        .unwrap_or_else(|| "unmatched".into());
    let resp = next.run(req).await;
    let status = resp.status();
    state.metrics.http_requests.add(
        &[
            ("method", &method),
            ("route", &route),
            ("status", status.as_str()),
        ],
        1,
    );
    state.metrics.http_latency.observe(started.elapsed());
    resp
}

/// Fallback for unknown routes.
pub async fn not_found() -> Response {
    Problem::new(StatusCode::NOT_FOUND, Some("route not found".into())).into_response()
}
