//! Realtime endpoints: tickets, SSE and WebSocket.
//!
//! `EventSource` and browser WebSockets cannot send headers, so clients
//! first trade their bearer token for a single-use ticket (30 s) bound to
//! one graph, then pass it as `?ticket=`.

use axum::Json;
use axum::extract::State;
use axum::extract::ws::WebSocketUpgrade;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::validation::{FieldErrors, Validate};
use crate::engine::editor;
use crate::http::extract::{AuthUser, Path, Query, ValidatedJson};
use crate::http::problem::Problem;
use crate::realtime::{sse, ws};
use crate::repo::tickets::{self, TICKET_TTL, TicketGrant};
use crate::repo::{self, OrNotFound};

/// `POST /realtime/tickets` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct TicketRequest {
    pub graph_id: Uuid,
}

impl Validate for TicketRequest {
    fn validate(&self, _errors: &mut FieldErrors) {}
}

/// A single-use realtime ticket.
#[derive(Debug, Serialize, ToSchema)]
pub struct TicketResponse {
    pub ticket: String,
    /// Seconds until the ticket expires.
    pub expires_in: u64,
}

/// `?ticket=` of the SSE and WebSocket endpoints.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TicketQuery {
    /// Ticket from `POST /realtime/tickets`.
    pub ticket: String,
}

/// Issues a single-use ticket for one of the caller's graphs.
#[utoipa::path(post, path = "/realtime/tickets", tag = "realtime", security(("bearer" = [])),
    request_body = TicketRequest,
    responses((status = 200, body = TicketResponse), (status = 404, body = Problem)))]
pub async fn ticket(
    State(state): State<AppState>,
    auth: AuthUser,
    ValidatedJson(req): ValidatedJson<TicketRequest>,
) -> Result<Json<TicketResponse>, AppError> {
    editor::owned_graph(&state, auth.id, req.graph_id).await?;
    let ticket = tickets::issue(
        &state.db,
        TicketGrant {
            user_id: auth.id,
            graph_id: req.graph_id,
        },
    )
    .await?;
    Ok(Json(TicketResponse {
        ticket,
        expires_in: TICKET_TTL.as_secs(),
    }))
}

/// Redeems a ticket for `graph_id` and re-checks that the graph is still the user's.
async fn redeem(state: &AppState, graph_id: Uuid, ticket: &str) -> Result<TicketGrant, AppError> {
    let grant = tickets::redeem(&state.db, ticket)
        .await?
        .ok_or(AppError::Unauthorized("invalid or expired ticket"))?;
    if grant.graph_id != graph_id {
        return Err(AppError::Unauthorized("ticket is not valid for this graph"));
    }
    editor::owned_graph(state, grant.user_id, graph_id).await?;
    Ok(grant)
}

/// Server-sent events of a graph (contract §6).
#[utoipa::path(get, path = "/graphs/{gid}/events", tag = "realtime",
    params(("gid" = Uuid, Path, description = "Graph id"), TicketQuery),
    responses(
        (status = 200, description = "SSE stream", content_type = "text/event-stream", body = String),
        (status = 401, body = Problem),
    ))]
pub async fn events(
    State(state): State<AppState>,
    Path(gid): Path<Uuid>,
    Query(q): Query<TicketQuery>,
) -> Result<Response, AppError> {
    let grant = redeem(&state, gid, &q.ticket).await?;
    Ok(sse::stream(state, gid, grant.user_id).into_response())
}

/// Collaborative WebSocket of a graph (contract §7). The `Origin` header,
/// when present, must be one of `NEXC_CORS_ORIGINS`.
#[utoipa::path(get, path = "/graphs/{gid}/ws", tag = "realtime",
    params(("gid" = Uuid, Path, description = "Graph id"), TicketQuery),
    responses(
        (status = 101, description = "Switching protocols"),
        (status = 401, body = Problem),
        (status = 403, description = "Origin not allowed", body = Problem),
    ))]
pub async fn websocket(
    State(state): State<AppState>,
    Path(gid): Path<Uuid>,
    Query(q): Query<TicketQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Result<Response, AppError> {
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok())
        && !state
            .settings
            .cors_origins
            .iter()
            .any(|allowed| allowed == origin)
    {
        return Err(AppError::Forbidden("origin not allowed".into()));
    }
    let grant = redeem(&state, gid, &q.ticket).await?;
    let user = repo::users::find(&state.db, grant.user_id)
        .await
        .or_not_found("user")?;
    let peer = ws::Peer {
        user_id: user.id,
        name: user.name,
        graph_id: gid,
    };
    Ok(upgrade
        .max_message_size(ws::MAX_FRAME_BYTES)
        .on_upgrade(move |socket| ws::session(state, socket, peer))
        .into_response())
}
