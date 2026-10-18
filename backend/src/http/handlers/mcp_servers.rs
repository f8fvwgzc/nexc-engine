//! The MCP servers of a workspace (the tools gateway): registered, tested
//! and removed by admins; listed to every member.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use uuid::Uuid;

use super::workspaces::{audit, member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::audit::AuditAction;
use crate::domain::mcp::{McpCheck, McpServer, McpServerInput};
use crate::domain::validation::{FieldErrors, Validate};
use crate::domain::workspace::WorkspaceAction;
use crate::gateway;
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo;
use crate::repo::audit::Subject;
use crate::repo::mcp_servers::NewMcpServer;

/// Most servers one workspace registers.
pub const SERVERS_MAX: i64 = 64;

impl Validate for McpServerInput {
    fn validate(&self, errors: &mut FieldErrors) {
        self.check(errors);
    }
}

/// The MCP servers of a workspace (members and above).
#[utoipa::path(get, path = "/workspaces/{wid}/mcp-servers", tag = "mcp-servers", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = [McpServer]), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Vec<McpServer>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::ViewMembers,
    )?;
    Ok(Json(
        repo::mcp_servers::list(&state.db, wid)
            .await?
            .into_iter()
            .map(|row| row.server)
            .collect(),
    ))
}

/// Registers an MCP server for the workspace's agents (admins). The server
/// is connected to at once and its tools recorded; a server that does not
/// answer is still registered, with the failure on it.
#[utoipa::path(post, path = "/workspaces/{wid}/mcp-servers", tag = "mcp-servers", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = McpServerInput,
    responses((status = 201, body = McpServer), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, body = Problem), (status = 422, body = Problem)))]
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<McpServerInput>,
) -> Result<(StatusCode, Json<McpServer>), AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let existing = repo::mcp_servers::list(&state.db, wid).await?;
    if existing.len() as i64 >= SERVERS_MAX {
        return Err(AppError::Unprocessable(format!(
            "a workspace registers at most {SERVERS_MAX} MCP servers"
        )));
    }
    let name = req.name.trim().to_owned();
    if existing.iter().any(|row| row.server.name == name) {
        return Err(AppError::Conflict(format!(
            "an MCP server named {name} is already registered"
        )));
    }
    let id = Uuid::now_v7();
    let sealed = match req
        .auth_header
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
    {
        Some(header) => Some(
            state
                .secret_box
                .seal(header.as_bytes(), id.as_bytes())
                .map_err(AppError::Internal)?,
        ),
        None => None,
    };
    let command = req.command.as_deref().map(str::trim).map(str::to_owned);
    let url = req.url.as_deref().map(str::trim).map(str::to_owned);
    let allowed_tools: Vec<String> = req
        .allowed_tools
        .iter()
        .map(|t| t.trim().to_owned())
        .collect();
    let row = repo::mcp_servers::insert(
        &state.db,
        id,
        &NewMcpServer {
            workspace_id: wid,
            name: &name,
            transport: req.transport,
            command: command.as_deref(),
            args: &req.args,
            url: url.as_deref(),
            auth_header: sealed.as_deref(),
            allowed_tools: &allowed_tools,
            max_label: req.max_label,
            created_by: auth.id,
        },
    )
    .await?;
    let check = gateway::check_server(&state, &row).await;
    state.gateway_registry.forget(wid);
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::McpServerAdded,
        Subject::Text(&format!("mcp:{name}")),
        &format!(
            "{} {}; {}",
            row.server.transport,
            row.server.host(),
            check.detail
        ),
    )
    .await;
    let server = repo::mcp_servers::find(&state.db, wid, id)
        .await?
        .map(|row| row.server)
        .ok_or(AppError::NotFound("mcp server"))?;
    Ok((StatusCode::CREATED, Json(server)))
}

/// One MCP server of the workspace.
#[utoipa::path(get, path = "/workspaces/{wid}/mcp-servers/{sid}", tag = "mcp-servers", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("sid" = Uuid, Path, description = "Server id")),
    responses((status = 200, body = McpServer), (status = 404, body = Problem)))]
pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, sid)): Path<(Uuid, Uuid)>,
) -> Result<Json<McpServer>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::ViewMembers,
    )?;
    let row = repo::mcp_servers::find(&state.db, wid, sid)
        .await?
        .ok_or(AppError::NotFound("mcp server"))?;
    Ok(Json(row.server))
}

/// Connects to a server again and records the tools it lists (admins).
#[utoipa::path(post, path = "/workspaces/{wid}/mcp-servers/{sid}/check", tag = "mcp-servers", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("sid" = Uuid, Path, description = "Server id")),
    responses((status = 200, body = McpCheck), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn check(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, sid)): Path<(Uuid, Uuid)>,
) -> Result<Json<McpCheck>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let row = repo::mcp_servers::find(&state.db, wid, sid)
        .await?
        .ok_or(AppError::NotFound("mcp server"))?;
    let found = gateway::check_server(&state, &row).await;
    state.gateway_registry.forget(wid);
    Ok(Json(found))
}

/// Removes a server (admins); its tools stop being offered at the next run.
#[utoipa::path(delete, path = "/workspaces/{wid}/mcp-servers/{sid}", tag = "mcp-servers", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id"), ("sid" = Uuid, Path, description = "Server id")),
    responses((status = 204, description = "Removed"), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path((wid, sid)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::UpdateSettings,
    )?;
    let row = repo::mcp_servers::find(&state.db, wid, sid)
        .await?
        .ok_or(AppError::NotFound("mcp server"))?;
    if !repo::mcp_servers::delete(&state.db, wid, sid).await? {
        return Err(AppError::NotFound("mcp server"));
    }
    state.gateway_registry.forget(wid);
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::McpServerRemoved,
        Subject::Text(&format!("mcp:{}", row.server.name)),
        &format!("{} {}", row.server.transport, row.server.host()),
    )
    .await;
    Ok(StatusCode::NO_CONTENT)
}
