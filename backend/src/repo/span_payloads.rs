//! What spans said, when a workspace records it (`span_payloads`): written
//! sealed, read by span, deleted past the workspace's retention.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::recording::PayloadKind;

/// A payload as stored: sealed content.
#[derive(Debug, Clone)]
pub struct PayloadRow {
    pub run_id: Uuid,
    pub kind: PayloadKind,
    pub content: Vec<u8>,
    pub size: i32,
    pub redactions: i32,
    pub created_at: DateTime<Utc>,
}

impl FromRow<'_, PgRow> for PayloadRow {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(PayloadRow {
            run_id: row.try_get("run_id")?,
            kind: enum_col(row, "kind")?,
            content: row.try_get("content")?,
            size: row.try_get("size")?,
            redactions: row.try_get("redactions")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// What to keep of a span.
#[derive(Debug, Clone)]
pub struct NewPayload<'a> {
    pub span_id: Uuid,
    pub run_id: Uuid,
    pub kind: PayloadKind,
    /// The content, sealed.
    pub sealed: &'a [u8],
    /// Bytes before sealing, after redaction.
    pub size: i32,
    pub redactions: i32,
}

/// Keeps a payload; a second payload of the same kind for a span is dropped.
pub async fn insert(db: impl PgExecutor<'_>, new: &NewPayload<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO span_payloads (span_id, kind, run_id, content, size, redactions)
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (span_id, kind) DO NOTHING",
    )
    .bind(new.span_id)
    .bind(new.kind.as_str())
    .bind(new.run_id)
    .bind(new.sealed)
    .bind(new.size)
    .bind(new.redactions)
    .execute(db)
    .await?;
    Ok(())
}

/// The payloads of a span, prompt first.
pub async fn of_span(
    db: impl PgExecutor<'_>,
    span_id: Uuid,
) -> Result<Vec<PayloadRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT run_id, kind, content, size, redactions, created_at FROM span_payloads
         WHERE span_id = $1
         ORDER BY array_position(ARRAY['prompt', 'completion', 'tool_input', 'tool_output'], kind)",
    )
    .bind(span_id)
    .fetch_all(db)
    .await
}

/// How many payloads a run has, by kind.
pub async fn count_for_run(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
) -> Result<Vec<(String, i64)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT kind, count(*) FROM span_payloads WHERE run_id = $1 GROUP BY kind ORDER BY kind",
    )
    .bind(run_id)
    .fetch_all(db)
    .await
}

/// Deletes every payload older than its workspace's `recording.retention_days`
/// (30 when the workspace set none); how many went.
pub async fn purge_expired(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "DELETE FROM span_payloads p
         USING runs r
         JOIN graphs g ON g.id = r.graph_id
         LEFT JOIN workspace_guardrails wg ON wg.workspace_id = g.workspace_id
         WHERE p.run_id = r.id
           AND p.created_at < now() - make_interval(days =>
               COALESCE((wg.config->'recording'->>'retention_days')::int, 30))",
    )
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}
