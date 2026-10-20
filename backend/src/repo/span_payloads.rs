//! What spans said, when a workspace records it (`span_payloads`): written
//! sealed, read by span, deleted past the workspace's retention
//! (`repo::evidence::purge_payloads`).

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::bundle::PayloadMeta;
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

/// What was recorded of a run's spans, without the content: for its evidence bundle.
pub async fn meta_for_run(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
) -> Result<Vec<PayloadMeta>, sqlx::Error> {
    let rows: Vec<(Uuid, String, i32, i32, DateTime<Utc>)> = sqlx::query_as(
        "SELECT span_id, kind, size, redactions, created_at FROM span_payloads
         WHERE run_id = $1 ORDER BY span_id, kind",
    )
    .bind(run_id)
    .fetch_all(db)
    .await?;
    rows.into_iter()
        .map(|(span_id, kind, size, redactions, created_at)| {
            Ok(PayloadMeta {
                span_id,
                kind: kind.parse().map_err(|_| sqlx::Error::ColumnDecode {
                    index: "kind".into(),
                    source: format!("unknown payload kind `{kind}`").into(),
                })?,
                size: size.into(),
                redactions: redactions.into(),
                created_at,
            })
        })
        .collect()
}
