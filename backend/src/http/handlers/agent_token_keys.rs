//! The public keys of the per-run agent tokens, as a JSON Web Key Set: what
//! the agent runtime and any external verifier check a token's signature
//! with, picked by the `kid` of its header.

use axum::Json;
use axum::extract::State;
use axum::http::header;
use axum::middleware::from_fn;
use axum::response::{IntoResponse, Response};

use crate::app::AppState;
use crate::http::middleware::etag::{PUBLIC_CACHE_CONTROL, conditional_reads};
use crate::security::agent_token::Jwks;

/// The keys agent tokens verify with: the one the backend signs with first,
/// then the others it still accepts (`NEXC_AGENT_TOKEN_VERIFY_KEYS`: the
/// previous key after a rotation, the next one before it). Public: they are
/// public keys, and the runtime fetches them without credentials; any cache
/// may keep them for five minutes (`PUBLIC_CACHE_CONTROL`).
#[utoipa::path(get, path = "/agent-tokens/jwks.json", tag = "agents",
    responses((status = 200, body = Jwks)))]
pub async fn jwks(State(state): State<AppState>) -> Response {
    (
        [(header::CACHE_CONTROL, PUBLIC_CACHE_CONTROL)],
        Json(state.agent_tokens.jwks()),
    )
        .into_response()
}

/// The router of `NEXC_KEYS_ADDR`: the key set at the same path as on the
/// API, and nothing else (every other path is 404), so a NetworkPolicy can
/// let the runtime reach it without opening the API to it. It is tagged as
/// on the API, so a verifier that keeps it asks again with `If-None-Match`.
pub fn keys_only_router(state: AppState) -> axum::Router {
    axum::Router::new()
        .route("/api/v1/agent-tokens/jwks.json", axum::routing::get(jwks))
        .layer(from_fn(conditional_reads))
        .with_state(state)
}
