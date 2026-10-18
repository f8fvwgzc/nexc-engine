//! The OTLP export's cursor over the trace spine (`otlp_export_cursor`) and
//! the spans past it, with the workspace each belongs to.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use crate::domain::trace::Span;

/// A span to export, with where it belongs.
#[derive(Debug, Clone)]
pub struct OtlpSpan {
    pub span: Span,
    pub ingested_seq: i64,
    pub workspace_id: Option<Uuid>,
    pub workspace_name: String,
}

impl FromRow<'_, PgRow> for OtlpSpan {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(OtlpSpan {
            span: Span::from_row(row)?,
            ingested_seq: row.try_get("ingested_seq")?,
            workspace_id: row.try_get("workspace_id")?,
            workspace_name: row.try_get("workspace_name")?,
        })
    }
}

/// Where the export stands.
#[derive(Debug, Clone, Default)]
pub struct CursorStatus {
    pub span_seq: i64,
    pub spans_exported: i64,
    /// Spans written that have not gone out yet.
    pub behind: i64,
    pub last_exported_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// Takes the cursor for one pass; `None` when another instance holds it.
/// Must run inside a transaction, which holds the row until it ends.
pub async fn claim(conn: &mut sqlx::PgConnection) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT span_seq FROM otlp_export_cursor WHERE id = 1 FOR UPDATE SKIP LOCKED",
    )
    .fetch_optional(conn)
    .await
}

/// Up to `limit` spans after `seq` in insertion order.
pub async fn spans_after(
    db: impl PgExecutor<'_>,
    seq: i64,
    limit: i64,
) -> Result<Vec<OtlpSpan>, sqlx::Error> {
    sqlx::query_as(
        "SELECT s.id, s.run_id, s.node_id, s.agent_id, s.principal, s.parent_id, s.kind, s.name,
                s.started_at, s.ended_at, s.status, s.attributes, s.seq, s.prev_hash, s.hash,
                s.ingested_seq, g.workspace_id, COALESCE(w.name, '') AS workspace_name
         FROM agent_spans s
         JOIN runs r ON r.id = s.run_id
         JOIN graphs g ON g.id = r.graph_id
         LEFT JOIN workspaces w ON w.id = g.workspace_id
         WHERE s.ingested_seq > $1
         ORDER BY s.ingested_seq LIMIT $2",
    )
    .bind(seq)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Moves the cursor past `seq` after `exported` spans went out.
pub async fn advance(db: impl PgExecutor<'_>, seq: i64, exported: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE otlp_export_cursor SET span_seq = $1, spans_exported = spans_exported + $2,
                last_exported_at = now(), last_error = NULL, updated_at = now()
         WHERE id = 1",
    )
    .bind(seq)
    .bind(exported)
    .execute(db)
    .await?;
    Ok(())
}

/// Notes why the last pass failed.
pub async fn note_error(db: impl PgExecutor<'_>, error: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE otlp_export_cursor SET last_error = $1, updated_at = now() WHERE id = 1")
        .bind(error.chars().take(1000).collect::<String>())
        .execute(db)
        .await?;
    Ok(())
}

/// Where the export stands, with how many spans are still to go.
pub async fn status(db: impl PgExecutor<'_> + Copy) -> Result<CursorStatus, sqlx::Error> {
    let row: (i64, i64, Option<DateTime<Utc>>, Option<String>) = sqlx::query_as(
        "SELECT span_seq, spans_exported, last_exported_at, last_error FROM otlp_export_cursor WHERE id = 1",
    )
    .fetch_one(db)
    .await?;
    let behind: i64 =
        sqlx::query_scalar("SELECT count(*) FROM agent_spans WHERE ingested_seq > $1")
            .bind(row.0)
            .fetch_one(db)
            .await?;
    Ok(CursorStatus {
        span_seq: row.0,
        spans_exported: row.1,
        behind,
        last_exported_at: row.2,
        last_error: row.3,
    })
}
