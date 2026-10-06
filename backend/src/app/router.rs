//! Assembles the axum router with all middleware.

use std::time::Duration;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode, header};
use axum::middleware::{from_fn, from_fn_with_state};
use axum::routing::get;
use tower_http::compression::CompressionLayer;
use tower_http::compression::predicate::{DefaultPredicate, NotForContentType, Predicate};
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use utoipa_scalar::{Scalar, Servable};

use super::AppState;
use crate::http::handlers::health;
use crate::http::{self, middleware};

/// Maximum request body (contract §3).
pub const BODY_LIMIT: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

fn cors(state: &AppState) -> CorsLayer {
    let origins: Vec<HeaderValue> = state
        .settings
        .cors_origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_credentials(true)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::IF_NONE_MATCH,
            HeaderName::from_static("x-requested-with"),
        ])
        .expose_headers([
            header::ETAG,
            header::RETRY_AFTER,
            HeaderName::from_static("x-request-id"),
        ])
        .max_age(Duration::from_secs(600))
}

/// The complete application router.
pub fn build(state: AppState) -> Router {
    let (api, spec) = http::api_router().split_for_parts();
    let spec_json = serde_json::to_string(&spec).expect("OpenAPI document serialises");
    let request_id = HeaderName::from_static("x-request-id");
    let compression = CompressionLayer::new().compress_when(
        DefaultPredicate::new().and(NotForContentType::const_new("text/event-stream")),
    );

    Router::new()
        .merge(api)
        .route("/healthz", get(health::healthz))
        .route("/readyz", get(health::readyz))
        .route(
            "/api/openapi.json",
            get(move || {
                let body = spec_json.clone();
                async move { ([(header::CONTENT_TYPE, "application/json")], body) }
            }),
        )
        .merge(Scalar::with_url("/api/docs", spec).title("nexc-engine API"))
        .fallback(middleware::not_found)
        .layer(from_fn_with_state(state.clone(), middleware::track_metrics))
        .layer(DefaultBodyLimit::max(BODY_LIMIT))
        .layer(from_fn(middleware::limit_body))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::GATEWAY_TIMEOUT,
            REQUEST_TIMEOUT,
        ))
        .layer(from_fn(middleware::problem_errors))
        .layer(from_fn_with_state(
            state.clone(),
            middleware::security_headers,
        ))
        .layer(compression)
        .layer(cors(&state))
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(TraceLayer::new_for_http())
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
        .with_state(state)
}
