//! MCP servers of a workspace (`mcp_servers`): registered by admins, called
//! by the gateway on an agent's behalf.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::capabilities::DataLabel;
use crate::domain::mcp::{McpServer, McpTool, McpTransport};

/// A server row with its encrypted header, for the gateway only.
#[derive(Debug, Clone)]
pub struct McpServerRow {
    pub server: McpServer,
    /// AES-256-GCM under the master key, bound to the server's id.
    pub auth_header: Option<Vec<u8>>,
}

fn strings(row: &PgRow, col: &str) -> Result<Vec<String>, sqlx::Error> {
    let raw: serde_json::Value = row.try_get(col)?;
    Ok(serde_json::from_value(raw).unwrap_or_default())
}

impl FromRow<'_, PgRow> for McpServerRow {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let tools: serde_json::Value = row.try_get("tools")?;
        let auth_header: Option<Vec<u8>> = row.try_get("auth_header")?;
        let last_checked_at: Option<DateTime<Utc>> = row.try_get("last_checked_at")?;
        Ok(McpServerRow {
            server: McpServer {
                id: row.try_get("id")?,
                workspace_id: row.try_get("workspace_id")?,
                name: row.try_get("name")?,
                transport: enum_col(row, "transport")?,
                command: row.try_get("command")?,
                args: strings(row, "args")?,
                url: row.try_get("url")?,
                has_auth_header: auth_header.is_some(),
                allowed_tools: strings(row, "allowed_tools")?,
                max_label: enum_col(row, "max_label")?,
                tools: serde_json::from_value::<Vec<McpTool>>(tools).unwrap_or_default(),
                last_checked_at,
                last_check_ok: row.try_get("last_check_ok")?,
                last_check_detail: row.try_get("last_check_detail")?,
                created_by: row.try_get("created_by")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            },
            auth_header,
        })
    }
}

/// What a server is registered with.
#[derive(Debug, Clone)]
pub struct NewMcpServer<'a> {
    pub workspace_id: Uuid,
    pub name: &'a str,
    pub transport: McpTransport,
    pub command: Option<&'a str>,
    pub args: &'a [String],
    pub url: Option<&'a str>,
    pub auth_header: Option<&'a [u8]>,
    pub allowed_tools: &'a [String],
    pub max_label: DataLabel,
    pub created_by: Uuid,
}

/// Registers a server; the id is chosen here so the header can be bound to it.
pub async fn insert(
    db: impl PgExecutor<'_>,
    id: Uuid,
    new: &NewMcpServer<'_>,
) -> Result<McpServerRow, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO mcp_servers (id, workspace_id, name, transport, command, args, url, auth_header,
                                  allowed_tools, max_label, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING *",
    )
    .bind(id)
    .bind(new.workspace_id)
    .bind(new.name)
    .bind(new.transport.as_str())
    .bind(new.command)
    .bind(serde_json::to_value(new.args).expect("args serialise"))
    .bind(new.url)
    .bind(new.auth_header)
    .bind(serde_json::to_value(new.allowed_tools).expect("tools serialise"))
    .bind(new.max_label.as_str())
    .bind(new.created_by)
    .fetch_one(db)
    .await
}

/// The servers of a workspace, by name.
pub async fn list(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<McpServerRow>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM mcp_servers WHERE workspace_id = $1 ORDER BY name")
        .bind(workspace_id)
        .fetch_all(db)
        .await
}

/// One server of a workspace.
pub async fn find(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<McpServerRow>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM mcp_servers WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// The server of a workspace named `name`.
pub async fn find_by_name(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    name: &str,
) -> Result<Option<McpServerRow>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM mcp_servers WHERE workspace_id = $1 AND name = $2")
        .bind(workspace_id)
        .bind(name)
        .fetch_optional(db)
        .await
}

/// Records what a connection test found.
pub async fn set_check(
    db: impl PgExecutor<'_>,
    id: Uuid,
    ok: bool,
    detail: &str,
    tools: &[McpTool],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE mcp_servers SET last_checked_at = now(), last_check_ok = $2, last_check_detail = $3,
                tools = CASE WHEN $2 THEN $4::jsonb ELSE tools END, updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(ok)
    .bind(detail)
    .bind(serde_json::to_value(tools).expect("tools serialise"))
    .execute(db)
    .await?;
    Ok(())
}

/// Removes a server; false when it was not there.
pub async fn delete(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM mcp_servers WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() == 1)
}
