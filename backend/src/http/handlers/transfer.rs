//! Transferring a workspace's data to a PostgreSQL its owner names.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json as Jsonb;
use utoipa::ToSchema;
use uuid::Uuid;

use super::workspaces::{audit, member_of, require};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::audit::AuditAction;
use crate::domain::validation::{FieldErrors, Validate};
use crate::domain::workspace::WorkspaceAction;
use crate::engine::transfer::{self, TableReport};
use crate::http::extract::{AuthUser, Path, ValidatedJson};
use crate::http::problem::Problem;
use crate::repo::audit::Subject;

/// A transfer of a workspace and how it went.
#[derive(Debug, Serialize, ToSchema)]
pub struct Transfer {
    pub id: Uuid,
    /// Where the data went, without credentials.
    pub target: String,
    /// The Redis it moved to, without credentials; empty when none was named.
    pub redis_target: String,
    /// `running`, `done` or `failed`.
    pub status: String,
    /// Per table: rows read here and rows written there.
    pub report: Vec<TableReport>,
    /// Why it failed; empty otherwise.
    pub error: String,
    pub created_at: DateTime<Utc>,
    #[schema(required = true)]
    pub finished_at: Option<DateTime<Utc>>,
}

type TransferRow = (
    Uuid,
    String,
    String,
    String,
    Jsonb<serde_json::Value>,
    String,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

fn transfer(row: TransferRow) -> Transfer {
    let (id, target, redis_target, status, Jsonb(report), error, created_at, finished_at) = row;
    let report = report
        .as_array()
        .into_iter()
        .flatten()
        .map(|t| TableReport {
            table: t["table"].as_str().unwrap_or_default().to_owned(),
            read: t["read"].as_i64().unwrap_or(0),
            written: t["written"].as_i64().unwrap_or(0),
        })
        .collect();
    Transfer {
        id,
        target,
        redis_target,
        status,
        report,
        error,
        created_at,
        finished_at,
    }
}

const COLUMNS: &str = "id, target, redis_target, status, report, error, created_at, finished_at";

/// The workspace's transfers, latest first (owners only).
#[utoipa::path(get, path = "/workspaces/{wid}/transfers", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, body = [Transfer]), (status = 403, body = Problem), (status = 404, body = Problem)))]
pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Json<Vec<Transfer>>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::Delete,
    )?;
    let rows: Vec<TransferRow> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "SELECT {COLUMNS} FROM workspace_transfers WHERE workspace_id = $1
         ORDER BY created_at DESC LIMIT 20"
    )))
    .bind(wid)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(rows.into_iter().map(transfer).collect()))
}

/// `POST /workspaces/{wid}/transfers` body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct StartTransfer {
    /// `postgres://user:password@host:5432/database` of the database that
    /// should receive the workspace. Used for this transfer only: it is
    /// neither stored nor logged. The user needs the right to create tables.
    pub url: String,
    /// `redis://[:password@]host:6379` the workspace moves to, if any. The
    /// server keeps nothing durable in Redis; it is checked and told of the
    /// handover. Neither stored nor logged.
    pub redis_url: Option<String>,
}

impl Validate for StartTransfer {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.url.len() > 2_000 {
            errors.add("url", "is too long");
        }
        if let Some(redis) = &self.redis_url {
            if !redis.starts_with("redis://") && !redis.starts_with("rediss://") {
                errors.add("redis_url", "must start with redis://");
            }
            if redis.len() > 2_000 {
                errors.add("redis_url", "is too long");
            }
        }
    }
}

/// What a connection check found, for each target the owner named.
#[derive(Debug, Serialize, ToSchema)]
pub struct TransferCheck {
    pub database_ok: bool,
    /// The database's version, or why it could not be reached.
    pub database: String,
    /// `null` when no Redis was named.
    #[schema(required = true)]
    pub redis_ok: Option<bool>,
    #[schema(required = true)]
    pub redis: Option<String>,
}

/// Checks that the database (and Redis, if named) can be reached from this
/// server and that the database is empty of this workspace (owners only).
/// Nothing is written and nothing is kept.
#[utoipa::path(post, path = "/workspaces/{wid}/transfers/check", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = StartTransfer,
    responses((status = 200, body = TransferCheck), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 422, body = Problem)))]
pub async fn check(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<StartTransfer>,
) -> Result<Json<TransferCheck>, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::Delete,
    )?;
    let production = state.settings.is_production();
    let database = match transfer::check_target(&req.url, production).await {
        Err(reason) => (false, reason),
        Ok(()) => match transfer::probe_target(&req.url, wid).await {
            Ok(version) => (true, version),
            Err(err) => (false, err.to_string()),
        },
    };
    let redis = match req.redis_url.as_deref() {
        None => (None, None),
        Some(url) => match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            super::infrastructure::check_redis(url),
        )
        .await
        {
            Ok(Ok(detail)) => (Some(true), Some(detail)),
            Ok(Err(err)) => (Some(false), Some(err.to_string())),
            Err(_) => (Some(false), Some("no answer within 5 seconds".into())),
        },
    };
    Ok(Json(TransferCheck {
        database_ok: database.0,
        database: database.1,
        redis_ok: redis.0,
        redis: redis.1,
    }))
}

/// Starts copying the workspace to the PostgreSQL at `url` (owners only):
/// its teams, members, issues, graphs, runs, documents, memory, settings and
/// logs. Password hashes and stored API keys are not copied. Nothing changes
/// on this server; the copy runs in the background and its outcome is read
/// from `GET …/transfers`. One transfer runs at a time.
#[utoipa::path(post, path = "/workspaces/{wid}/transfers", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")), request_body = StartTransfer,
    responses((status = 202, body = Transfer), (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 409, description = "A transfer is already running", body = Problem), (status = 422, body = Problem)))]
pub async fn start(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
    ValidatedJson(req): ValidatedJson<StartTransfer>,
) -> Result<(StatusCode, Json<Transfer>), AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::Delete,
    )?;
    let url = req.url.trim().to_owned();
    transfer::check_target(&url, state.settings.is_production())
        .await
        .map_err(|reason| AppError::field("url", reason))?;
    // A transfer whose server went away would block the next one for ever.
    sqlx::query(
        "UPDATE workspace_transfers SET status = 'failed', error = 'interrupted', finished_at = now()
         WHERE workspace_id = $1 AND status = 'running' AND created_at < now() - interval '6 hours'",
    )
    .bind(wid)
    .execute(&state.db)
    .await?;
    let running: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM workspace_transfers
                        WHERE workspace_id = $1 AND status = 'running')",
    )
    .bind(wid)
    .fetch_one(&state.db)
    .await?;
    if running {
        return Err(AppError::Conflict(
            "a transfer of this workspace is already running".into(),
        ));
    }
    let target = transfer::location(&url);
    let row: TransferRow = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "INSERT INTO workspace_transfers (id, workspace_id, started_by, target, redis_target)
         VALUES ($1, $2, $3, $4, $5) RETURNING {COLUMNS}"
    )))
    .bind(Uuid::now_v7())
    .bind(wid)
    .bind(auth.id)
    .bind(&target)
    .bind(
        req.redis_url
            .as_deref()
            .map(transfer::location)
            .unwrap_or_default(),
    )
    .fetch_one(&state.db)
    .await?;
    let started = transfer(row);
    let subject = Subject::Text("Workspace data");
    audit(
        &state,
        wid,
        auth.id,
        AuditAction::WorkspaceTransferred,
        subject,
        &target,
    )
    .await;
    let id = started.id;
    let redis_url = req.redis_url.clone();
    tokio::spawn(async move { transfer::run(&state, id, wid, url, redis_url).await });
    Ok((StatusCode::ACCEPTED, Json(started)))
}
