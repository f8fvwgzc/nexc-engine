//! Transferring a workspace's data to a PostgreSQL its owner names.

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
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
    Jsonb<serde_json::Value>,
    String,
    DateTime<Utc>,
    Option<DateTime<Utc>>,
);

fn transfer(row: TransferRow) -> Transfer {
    let (id, target, status, Jsonb(report), error, created_at, finished_at) = row;
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
        status,
        report,
        error,
        created_at,
        finished_at,
    }
}

const COLUMNS: &str = "id, target, status, report, error, created_at, finished_at";

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
}

impl Validate for StartTransfer {
    fn validate(&self, errors: &mut FieldErrors) {
        if self.url.len() > 2_000 {
            errors.add("url", "is too long");
        }
    }
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
        "INSERT INTO workspace_transfers (id, workspace_id, started_by, target)
         VALUES ($1, $2, $3, $4) RETURNING {COLUMNS}"
    )))
    .bind(Uuid::now_v7())
    .bind(wid)
    .bind(auth.id)
    .bind(&target)
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
    tokio::spawn(async move { transfer::run(&state, id, wid, url).await });
    Ok((StatusCode::ACCEPTED, Json(started)))
}

/// The largest set of files offered as one download. A workspace with more
/// is moved by copying the server's data folder itself.
const FILES_MAX_BYTES: i64 = 1024 * 1024 * 1024;

/// The files of a workspace that are kept on disk rather than in the
/// database, as one zip archive (owners only): the uploaded originals of its
/// documents under `documents/<document id>` and the artifacts of its runs
/// under `artifacts/<run id>/<node id>/<path>`. That is the layout of the
/// server's data folder: unpacked into the data folder of the server that
/// received the workspace's rows, the files are where it expects them.
#[utoipa::path(get, path = "/workspaces/{wid}/files.zip", tag = "workspaces", security(("bearer" = [])),
    params(("wid" = Uuid, Path, description = "Workspace id")),
    responses((status = 200, description = "Zip archive", content_type = "application/zip", body = Vec<u8>),
        (status = 403, body = Problem), (status = 404, body = Problem),
        (status = 413, description = "More than 1 GiB of files", body = Problem)))]
pub async fn files(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(wid): Path<Uuid>,
) -> Result<Response, AppError> {
    require(
        &member_of(&state, auth, wid).await?,
        WorkspaceAction::Delete,
    )?;
    let documents: Vec<(Uuid, i64)> =
        sqlx::query_as("SELECT id, size_bytes FROM documents WHERE workspace_id = $1 ORDER BY id")
            .bind(wid)
            .fetch_all(&state.db)
            .await?;
    let artifacts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT a.storage_path, a.size FROM artifacts a
         JOIN runs r ON r.id = a.run_id JOIN graphs g ON g.id = r.graph_id
         WHERE g.workspace_id = $1 ORDER BY a.storage_path",
    )
    .bind(wid)
    .fetch_all(&state.db)
    .await?;
    let total: i64 =
        documents.iter().map(|d| d.1).sum::<i64>() + artifacts.iter().map(|a| a.1).sum::<i64>();
    if total > FILES_MAX_BYTES {
        return Err(AppError::PayloadTooLarge);
    }
    // (name in the archive, file on disk)
    let mut entries = Vec::with_capacity(documents.len() + artifacts.len());
    for (id, _) in &documents {
        let on_disk = state.settings.documents_dir().join(id.to_string());
        entries.push((format!("documents/{id}"), on_disk));
    }
    for (storage_path, _) in &artifacts {
        // A stored path that would leave the artifacts folder is skipped, not followed.
        if let Ok(on_disk) = crate::engine::artifacts::resolve(&state, storage_path) {
            entries.push((format!("artifacts/{storage_path}"), on_disk));
        }
    }
    // Written to a file, not to memory, and stored without compression: PDFs and Office
    // files are compressed already, and the archive must be ready within the request timeout.
    let scratch = state.settings.data_dir.join("tmp");
    tokio::fs::create_dir_all(&scratch)
        .await
        .map_err(anyhow::Error::from)?;
    let archive = scratch.join(format!("{}.zip", Uuid::now_v7()));
    let target = archive.clone();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        use zip::write::SimpleFileOptions;
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&target)?);
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .large_file(true);
        for (name, on_disk) in entries {
            // A file that is gone from disk is left out; its record was transferred anyway.
            let Ok(mut file) = std::fs::File::open(&on_disk) else {
                continue;
            };
            writer.start_file(name, options)?;
            std::io::copy(&mut file, &mut writer)?;
        }
        writer.finish()?;
        Ok(())
    })
    .await
    .map_err(anyhow::Error::from)??;
    let file = tokio::fs::File::open(&archive)
        .await
        .map_err(anyhow::Error::from)?;
    // The open handle keeps the bytes readable; the name can go now.
    let _ = tokio::fs::remove_file(&archive).await;
    let headers = [
        (
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/zip"),
        ),
        (
            header::CONTENT_DISPOSITION,
            HeaderValue::from_static("attachment; filename=\"workspace-files.zip\""),
        ),
    ];
    let body = Body::from_stream(tokio_util::io::ReaderStream::new(file));
    Ok((headers, body).into_response())
}
