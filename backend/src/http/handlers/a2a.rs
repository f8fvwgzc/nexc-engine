//! The A2A surface: the agent card of an exposed agent (public, at the
//! well-known address and under `/a2a`), the JSON-RPC endpoint registered
//! counterparts send tasks to (bearer agent API token), the artifacts of a
//! task for its caller, and the inbound tasks of an agent for the console.

use axum::Json;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::sse::{KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::a2a::{self, inbound};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::a2a::{
    A2aTask, E_INVALID_REQUEST, E_METHOD_NOT_FOUND, E_PARSE, E_PUSH_UNSUPPORTED, RpcError,
};
use crate::engine::artifacts;
use crate::http::extract::{AuthUser, Path, Query};
use crate::http::problem::Problem;
use crate::repo;

/// Largest JSON-RPC request (file parts travel inline as base64).
pub const RPC_BODY_LIMIT: usize = 16 * 1024 * 1024;
/// Inbound tasks listed by default.
pub const TASKS_DEFAULT: i64 = 50;
/// Most inbound tasks listed at once.
pub const TASKS_MAX: i64 = 200;

/// The A2A agent card (`AgentCard` of the A2A specification), as served to anyone.
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct AgentCardDocument {
    pub name: String,
    pub description: String,
    /// The JSON-RPC endpoint tasks are sent to.
    pub url: String,
    #[schema(value_type = Object)]
    pub provider: Value,
    pub version: String,
    #[schema(value_type = Object)]
    pub capabilities: Value,
    #[schema(value_type = Object)]
    pub authentication: Value,
    #[schema(value_type = Vec<String>)]
    pub default_input_modes: Value,
    #[schema(value_type = Vec<String>)]
    pub default_output_modes: Value,
    #[schema(value_type = Vec<Object>)]
    pub skills: Value,
    /// The agent's principal in nexc.
    pub principal: String,
}

/// Query of `GET /.well-known/agent.json`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct WellKnownQuery {
    /// The exposed agent: its principal (`agent://{workspace}/{name}`) or its id.
    pub agent: String,
}

async fn card_response(state: &AppState, address: &str) -> Result<Response, AppError> {
    let (agent, workspace) = a2a::exposed_agent(state, address, None)
        .await
        .map_err(|_| AppError::NotFound("exposed agent"))?;
    let card = a2a::card::build(state, &agent, workspace).await?;
    Ok(Json(card).into_response())
}

/// The agent card of an exposed agent, at the well-known address. Public:
/// an agent that is not exposed is not found.
#[utoipa::path(get, path = "/.well-known/agent.json", tag = "a2a", params(WellKnownQuery),
    responses((status = 200, body = AgentCardDocument), (status = 404, body = Problem)))]
pub async fn well_known(
    State(state): State<AppState>,
    Query(query): Query<WellKnownQuery>,
) -> Result<Response, AppError> {
    card_response(&state, &query.agent).await
}

/// The agent card of an exposed agent, under its principal (percent-encoded)
/// or its id. Public: an agent that is not exposed is not found.
#[utoipa::path(get, path = "/a2a/agents/{principal}/card", tag = "a2a",
    params(("principal" = String, Path, description = "The agent's principal, percent-encoded, or its id")),
    responses((status = 200, body = AgentCardDocument), (status = 404, body = Problem)))]
pub async fn card(
    State(state): State<AppState>,
    Path(principal): Path<String>,
) -> Result<Response, AppError> {
    card_response(&state, &principal).await
}

/// A JSON-RPC request as the endpoint reads it.
#[derive(Debug, Deserialize, ToSchema)]
pub struct RpcRequest {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    #[schema(value_type = Object)]
    pub id: Value,
    /// `tasks/send`, `tasks/sendSubscribe`, `tasks/get`, `tasks/cancel`, `tasks/resubscribe`.
    pub method: String,
    #[schema(value_type = Object)]
    pub params: Value,
}

/// A JSON-RPC response: `result` on success, `error` otherwise.
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct RpcResponse {
    pub jsonrpc: String,
    #[schema(value_type = Object)]
    pub id: Value,
    #[schema(value_type = Object)]
    pub result: Option<Value>,
    #[schema(value_type = Object)]
    pub error: Option<Value>,
}

fn rpc_error(id: &Value, error: RpcError) -> Response {
    let status = StatusCode::from_u16(error.status).unwrap_or(StatusCode::OK);
    let body = json!({"jsonrpc": "2.0", "id": id, "error": error.to_value()});
    (status, Json(body)).into_response()
}

fn rpc_ok(id: &Value, result: Value) -> Response {
    Json(json!({"jsonrpc": "2.0", "id": id, "result": result})).into_response()
}

/// The JSON-RPC endpoint of an exposed agent (A2A): `tasks/send` runs the
/// task as a one-node run of the agent and answers once it ended or after
/// 25 s with it still working; `tasks/sendSubscribe` streams its status and
/// artifacts as server-sent events; `tasks/get`, `tasks/cancel` and
/// `tasks/resubscribe` take the task's id. The caller is a registered
/// counterpart presenting an agent API token; every task is a `tool_call`
/// span marked inbound and a `policy` span on the run's trace, and a loop or
/// a hop past the delegation depth is refused.
#[utoipa::path(post, path = "/a2a/agents/{principal}", tag = "a2a",
    params(("principal" = String, Path, description = "The agent's principal, percent-encoded, or its id")),
    request_body = RpcRequest,
    responses((status = 200, description = "A JSON-RPC response (or an event stream for tasks/sendSubscribe)", body = RpcResponse),
        (status = 401, description = "No usable agent API token (a JSON-RPC error)", body = RpcResponse),
        (status = 403, description = "The token's agent is not a counterpart, or is out of service", body = RpcResponse),
        (status = 404, description = "No exposed agent at that address", body = RpcResponse)))]
pub async fn rpc(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(principal): Path<String>,
    body: Bytes,
) -> Response {
    let caller = match a2a::authenticate(&state, &headers).await {
        Ok(caller) => caller,
        Err(err) => return rpc_error(&Value::Null, err),
    };
    let (agent, workspace) =
        match a2a::exposed_agent(&state, &principal, Some(caller.workspace_id)).await {
            Ok(found) => found,
            Err(err) => return rpc_error(&Value::Null, err),
        };
    let request: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(err) => {
            return rpc_error(
                &Value::Null,
                RpcError::new(E_PARSE, format!("the request is not JSON: {err}")),
            );
        }
    };
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let Some(object) = request.as_object() else {
        return rpc_error(
            &id,
            RpcError::new(E_INVALID_REQUEST, "a JSON-RPC request is an object"),
        );
    };
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return rpc_error(
            &id,
            RpcError::new(E_INVALID_REQUEST, "`jsonrpc` must be \"2.0\""),
        );
    }
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return rpc_error(
            &id,
            RpcError::new(E_INVALID_REQUEST, "`method` is required"),
        );
    };
    let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
    let outcome = match method {
        "tasks/send" => {
            let inbound = match inbound::Inbound::parse(&params) {
                Ok(inbound) => inbound,
                Err(err) => return rpc_error(&id, err),
            };
            match inbound::send(&state, &caller, &agent, workspace, inbound, false).await {
                Ok(accepted) => {
                    inbound::wait(&state, accepted.row.task.run_id, inbound::SEND_WAIT).await;
                    inbound::task_json(&state, &accepted.row, None).await
                }
                Err(err) => Err(err),
            }
        }
        "tasks/sendSubscribe" => {
            let inbound = match inbound::Inbound::parse(&params) {
                Ok(inbound) => inbound,
                Err(err) => return rpc_error(&id, err),
            };
            return match inbound::send(&state, &caller, &agent, workspace, inbound, true).await {
                Ok(accepted) => Sse::new(inbound::stream(state.clone(), accepted, id))
                    .keep_alive(KeepAlive::default())
                    .into_response(),
                Err(err) => rpc_error(&id, err),
            };
        }
        "tasks/get" | "tasks/cancel" | "tasks/resubscribe" => {
            let task_id = params
                .get("id")
                .and_then(Value::as_str)
                .and_then(|s| s.parse::<Uuid>().ok());
            let Some(task_id) = task_id else {
                return rpc_error(
                    &id,
                    RpcError::invalid_params("`id` must be the task's UUID"),
                );
            };
            match inbound::task_of(&state, &caller, task_id).await {
                Ok(row) if method == "tasks/cancel" => match inbound::cancel(&state, &row).await {
                    Ok(()) => inbound::task_json(&state, &row, Some(0)).await,
                    Err(err) => Err(err),
                },
                Ok(row) if method == "tasks/resubscribe" => {
                    let run = repo::runs::find_row(&state.db, row.task.run_id).await;
                    let (graph_id, live) = match run {
                        Ok(Some(run)) => (run.graph_id, !run.status.is_terminal()),
                        _ => {
                            return rpc_error(
                                &id,
                                RpcError::new(
                                    crate::domain::a2a::E_TASK_NOT_FOUND,
                                    "the task's run is gone",
                                ),
                            );
                        }
                    };
                    let accepted = inbound::Accepted {
                        events: live.then(|| state.hub.subscribe_sse(graph_id)),
                        row,
                        graph_id,
                    };
                    return Sse::new(inbound::stream(state.clone(), accepted, id))
                        .keep_alive(KeepAlive::default())
                        .into_response();
                }
                Ok(row) => {
                    let history = params.get("historyLength").and_then(Value::as_u64);
                    inbound::task_json(&state, &row, history).await
                }
                Err(err) => Err(err),
            }
        }
        "tasks/pushNotification/set" | "tasks/pushNotification/get" => Err(RpcError::new(
            E_PUSH_UNSUPPORTED,
            "push notifications are not supported",
        )),
        other => Err(RpcError::new(
            E_METHOD_NOT_FOUND,
            format!(
                "method `{}` is not one of tasks/send, tasks/sendSubscribe, tasks/get, tasks/cancel, tasks/resubscribe",
                other
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(64)
                    .collect::<String>()
            ),
        )),
    };
    match outcome {
        Ok(result) => rpc_ok(&id, result),
        Err(err) => rpc_error(&id, err),
    }
}

/// An artifact of a task, for the counterpart that sent it (bearer agent
/// API token): the file, with `Content-Disposition: attachment`. A file the
/// DLP classifier kept in is not found until an admin releases it.
#[utoipa::path(get, path = "/a2a/tasks/{tid}/artifacts/{aid}", tag = "a2a",
    params(("tid" = Uuid, Path, description = "Task id"), ("aid" = Uuid, Path, description = "Artifact id")),
    responses((status = 200, description = "The file", content_type = "application/octet-stream", body = Vec<u8>),
        (status = 401, body = RpcResponse), (status = 403, body = RpcResponse), (status = 404, body = RpcResponse)))]
pub async fn artifact(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path((tid, aid)): Path<(Uuid, Uuid)>,
) -> Response {
    let caller = match a2a::authenticate(&state, &headers).await {
        Ok(caller) => caller,
        Err(err) => return rpc_error(&Value::Null, err),
    };
    let not_found = || {
        rpc_error(
            &Value::Null,
            RpcError::new(crate::domain::a2a::E_TASK_NOT_FOUND, "no such artifact")
                .with_status(404),
        )
    };
    let row = match inbound::task_of(&state, &caller, tid).await {
        Ok(row) => row,
        Err(_) => return not_found(),
    };
    let stored = match repo::artifacts::list_stored(&state.db, row.task.run_id).await {
        Ok(all) => all.into_iter().find(|s| s.artifact.id == aid),
        Err(err) => return rpc_error(&Value::Null, a2a::internal(err)),
    };
    let Some(stored) =
        stored.filter(|s| s.artifact.quarantined_at.is_none() || s.artifact.released_at.is_some())
    else {
        return not_found();
    };
    let Ok(path) = artifacts::resolve(&state, &stored.storage_path) else {
        return not_found();
    };
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return not_found();
    };
    let name = stored
        .artifact
        .path
        .rsplit('/')
        .next()
        .unwrap_or("artifact")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let mut response = bytes.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&stored.artifact.mime)
            .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{name}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    response
}

/// Query of `GET /agents/{id}/a2a/tasks`.
#[derive(Debug, Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
pub struct TasksQuery {
    /// Tasks to return, 1–200 (default 50).
    pub limit: Option<i64>,
}

/// The tasks an exposed agent received over A2A, newest first (members of
/// its workspace): who called, the run that answered, the hop and the chain,
/// and the rule that refused it when the firewall did.
#[utoipa::path(get, path = "/agents/{id}/a2a/tasks", tag = "a2a", security(("bearer" = [])),
    params(("id" = Uuid, Path, description = "Agent id"), TasksQuery),
    responses((status = 200, body = [A2aTask]), (status = 404, body = Problem)))]
pub async fn tasks(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Query(query): Query<TasksQuery>,
) -> Result<Json<Vec<A2aTask>>, AppError> {
    super::agents::readable_agent(&state, auth, id).await?;
    let limit = query.limit.unwrap_or(TASKS_DEFAULT).clamp(1, TASKS_MAX);
    let rows = repo::a2a::list_tasks_for_agent(&state.db, id, limit).await?;
    Ok(Json(rows.into_iter().map(|row| row.task).collect()))
}
