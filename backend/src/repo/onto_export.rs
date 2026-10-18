//! What the onto-intelli export has to do and how far it got: the outbox the
//! tables fill (`onto_export_queue`), the per-workspace cursor of the backfill
//! (`onto_export_cursors`), and the rows the facts are built from.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::agent::AgentStatus;
use crate::domain::run::RunStatus;
use crate::domain::trace::Span;

/// Where a workspace's export stands.
#[derive(Debug, Clone, FromRow)]
pub struct Cursor {
    pub workspace_id: Uuid,
    pub span_seq: i64,
    pub backfill_before: Option<i64>,
    pub backfilled_at: Option<DateTime<Utc>>,
    pub spans_exported: i64,
    pub records_exported: i64,
    pub last_exported_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// The cursor of a workspace, if it was ever exported.
pub async fn cursor(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Option<Cursor>, sqlx::Error> {
    sqlx::query_as(
        "SELECT workspace_id, span_seq, backfill_before, backfilled_at, spans_exported,
                records_exported, last_exported_at, last_error
         FROM onto_export_cursors WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .fetch_optional(db)
    .await
}

/// Makes sure a workspace has a cursor row (at the start: nothing exported).
pub async fn ensure_cursor(db: impl PgExecutor<'_>, workspace_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO onto_export_cursors (workspace_id) VALUES ($1) ON CONFLICT DO NOTHING",
    )
    .bind(workspace_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Where a workspace's backfill stops: the sequence of the first span the
/// queue holds for it, or one past the last span there is. Everything below
/// is the backfill's, everything from there on the queue's.
pub async fn backfill_boundary(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT COALESCE(
             (SELECT min(s.ingested_seq) FROM onto_export_queue q JOIN agent_spans s ON s.id = q.subject_id
               WHERE q.kind = 'span' AND q.workspace_id = $1),
             (SELECT COALESCE(max(ingested_seq), 0) + 1 FROM agent_spans))",
    )
    .bind(workspace_id)
    .fetch_one(db)
    .await
}

/// Fixes where the backfill stops (once; later calls keep the first value).
pub async fn set_backfill_boundary(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    before: i64,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE onto_export_cursors SET backfill_before = COALESCE(backfill_before, $2), updated_at = now()
         WHERE workspace_id = $1 RETURNING backfill_before",
    )
    .bind(workspace_id)
    .bind(before)
    .fetch_one(db)
    .await
}

/// Workspaces with something to export: queued rows, or no completed
/// backfill while they have spans, runs or agents.
pub async fn workspaces_behind(db: impl PgExecutor<'_>) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT DISTINCT workspace_id FROM (
             SELECT workspace_id FROM onto_export_queue
             UNION
             SELECT w.id FROM workspaces w
              LEFT JOIN onto_export_cursors c ON c.workspace_id = w.id
              WHERE c.backfilled_at IS NULL
                AND (EXISTS (SELECT 1 FROM agents a WHERE a.workspace_id = w.id)
                     OR EXISTS (SELECT 1 FROM graphs g JOIN runs r ON r.graph_id = g.id
                                 WHERE g.workspace_id = w.id))
         ) behind ORDER BY workspace_id",
    )
    .fetch_all(db)
    .await
}

/// Pending work of a workspace, claimed for the calling transaction: rows
/// are locked until it ends, so another instance skips them; they are deleted
/// with [`finish`] once onto-intelli acknowledged the export, and come back
/// if the transaction rolls back.
#[derive(Debug, Clone)]
pub struct Claimed {
    pub ids: Vec<i64>,
    pub spans: Vec<Uuid>,
    pub runs: Vec<Uuid>,
    pub agents: Vec<Uuid>,
}

pub async fn claim(
    tx: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    limit: i64,
) -> Result<Claimed, sqlx::Error> {
    let rows: Vec<(i64, String, Uuid)> = sqlx::query_as(
        "SELECT id, kind, subject_id FROM onto_export_queue
         WHERE workspace_id = $1 ORDER BY id LIMIT $2 FOR UPDATE SKIP LOCKED",
    )
    .bind(workspace_id)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await?;
    let mut claimed = Claimed {
        ids: Vec::with_capacity(rows.len()),
        spans: Vec::new(),
        runs: Vec::new(),
        agents: Vec::new(),
    };
    for (id, kind, subject) in rows {
        claimed.ids.push(id);
        match kind.as_str() {
            "span" => claimed.spans.push(subject),
            "run" => claimed.runs.push(subject),
            _ => claimed.agents.push(subject),
        }
    }
    Ok(claimed)
}

/// Removes claimed rows: the export they stood for is in.
pub async fn finish(tx: &mut sqlx::PgConnection, ids: &[i64]) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM onto_export_queue WHERE id = ANY($1)")
        .bind(ids)
        .execute(tx)
        .await?;
    Ok(())
}

/// Rows queued across all workspaces.
pub async fn queued(db: impl PgExecutor<'_>) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM onto_export_queue")
        .fetch_one(db)
        .await
}

/// Empties the queue: nothing consumes it while the feature is off.
pub async fn purge(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query("DELETE FROM onto_export_queue")
        .execute(db)
        .await?
        .rows_affected())
}

/// A span with its insertion sequence and its run's graph.
#[derive(Debug, Clone)]
pub struct ExportSpan {
    pub span: Span,
    pub seq: i64,
    pub graph_id: Uuid,
}

impl FromRow<'_, PgRow> for ExportSpan {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(ExportSpan {
            span: Span::from_row(row)?,
            seq: row.try_get("ingested_seq")?,
            graph_id: row.try_get("graph_id")?,
        })
    }
}

/// The span columns with their insertion sequence and graph (SQL stays static).
macro_rules! span_select {
    () => {
        "SELECT s.id, s.run_id, s.node_id, s.agent_id, s.principal, s.parent_id, s.kind, s.name,
         s.started_at, s.ended_at, s.status, s.attributes, s.seq, s.prev_hash, s.hash,
         s.ingested_seq, r.graph_id
         FROM agent_spans s JOIN runs r ON r.id = s.run_id JOIN graphs g ON g.id = r.graph_id"
    };
}

/// The backfill: spans of a workspace in insertion order, after `seq` and
/// before `before`.
pub async fn spans_after(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    seq: i64,
    before: i64,
    limit: i64,
) -> Result<Vec<ExportSpan>, sqlx::Error> {
    sqlx::query_as(concat!(
        span_select!(),
        " WHERE g.workspace_id = $1 AND s.ingested_seq > $2 AND s.ingested_seq < $3
          ORDER BY s.ingested_seq LIMIT $4"
    ))
    .bind(workspace_id)
    .bind(seq)
    .bind(before)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Spans by id, in insertion order (the queue's).
pub async fn spans_by_id(
    db: impl PgExecutor<'_>,
    ids: &[Uuid],
) -> Result<Vec<ExportSpan>, sqlx::Error> {
    sqlx::query_as(concat!(
        span_select!(),
        " WHERE s.id = ANY($1) ORDER BY s.ingested_seq"
    ))
    .bind(ids)
    .fetch_all(db)
    .await
}

/// A run as the facts describe it: status, cost and tokens, never outputs.
#[derive(Debug, Clone)]
pub struct ExportRun {
    pub id: Uuid,
    pub graph_id: Uuid,
    pub status: RunStatus,
    pub cost_usd: f64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub policy_mismatches: i32,
    /// The queue holds this run's finish too: the backfill leaves the event to it.
    pub queued: bool,
}

impl FromRow<'_, PgRow> for ExportRun {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(ExportRun {
            id: row.try_get("id")?,
            graph_id: row.try_get("graph_id")?,
            status: enum_col(row, "status")?,
            cost_usd: row.try_get("cost_usd")?,
            tokens_in: row.try_get("tokens_in")?,
            tokens_out: row.try_get("tokens_out")?,
            started_at: row.try_get("started_at")?,
            finished_at: row.try_get("finished_at")?,
            policy_mismatches: row.try_get("policy_mismatches")?,
            queued: row.try_get("queued")?,
        })
    }
}

macro_rules! run_select {
    () => {
        "SELECT r.id, r.graph_id, r.status, r.cost_usd, r.tokens_in, r.tokens_out, r.started_at,
         r.finished_at, r.policy_mismatches,
         EXISTS (SELECT 1 FROM onto_export_queue q WHERE q.kind = 'run' AND q.subject_id = r.id) AS queued
         FROM runs r"
    };
}

pub async fn runs_by_id(
    db: impl PgExecutor<'_>,
    ids: &[Uuid],
) -> Result<Vec<ExportRun>, sqlx::Error> {
    sqlx::query_as(concat!(
        run_select!(),
        " WHERE r.id = ANY($1) ORDER BY r.id"
    ))
    .bind(ids)
    .fetch_all(db)
    .await
}

/// The backfill: every run of a workspace.
pub async fn runs_of(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<ExportRun>, sqlx::Error> {
    sqlx::query_as(concat!(
        run_select!(),
        " JOIN graphs g ON g.id = r.graph_id WHERE g.workspace_id = $1 ORDER BY r.id"
    ))
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// An agent as the facts describe it: its identity and its policy, never its prompt.
#[derive(Debug, Clone)]
pub struct ExportAgent {
    pub id: Uuid,
    pub principal_id: String,
    pub name: String,
    pub role: String,
    pub model: String,
    pub status: AgentStatus,
    pub policy_version: i32,
    pub capabilities: serde_json::Value,
    pub reports_to: Option<Uuid>,
}

impl FromRow<'_, PgRow> for ExportAgent {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(ExportAgent {
            id: row.try_get("id")?,
            principal_id: row.try_get("principal_id")?,
            name: row.try_get("name")?,
            role: row.try_get("role")?,
            model: row.try_get("model")?,
            status: enum_col(row, "status")?,
            policy_version: row.try_get("policy_version")?,
            capabilities: row.try_get("capabilities")?,
            reports_to: row.try_get("reports_to")?,
        })
    }
}

macro_rules! agent_select {
    () => {
        "SELECT id, principal_id, name, role, model, status, policy_version, capabilities, reports_to
         FROM agents"
    };
}

pub async fn agents_by_id(
    db: impl PgExecutor<'_>,
    ids: &[Uuid],
) -> Result<Vec<ExportAgent>, sqlx::Error> {
    sqlx::query_as(concat!(agent_select!(), " WHERE id = ANY($1) ORDER BY id"))
        .bind(ids)
        .fetch_all(db)
        .await
}

pub async fn agents_of(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<ExportAgent>, sqlx::Error> {
    sqlx::query_as(concat!(
        agent_select!(),
        " WHERE workspace_id = $1 ORDER BY id"
    ))
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// The documents behind passage ids, as `(passage, document, label)`.
pub async fn documents_of_chunks(
    db: impl PgExecutor<'_>,
    chunk_ids: &[Uuid],
) -> Result<Vec<(Uuid, Uuid, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT c.id, d.id, d.label FROM document_chunks c JOIN documents d ON d.id = c.document_id
         WHERE c.id = ANY($1) ORDER BY c.id",
    )
    .bind(chunk_ids)
    .fetch_all(db)
    .await
}

/// Documents by id, with their classification.
pub async fn document_labels(
    db: impl PgExecutor<'_>,
    ids: &[Uuid],
) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    sqlx::query_as("SELECT id, label FROM documents WHERE id = ANY($1) ORDER BY id")
        .bind(ids)
        .fetch_all(db)
        .await
}

/// Memories by id, with their classification.
pub async fn memory_labels(
    db: impl PgExecutor<'_>,
    ids: &[Uuid],
) -> Result<Vec<(Uuid, String)>, sqlx::Error> {
    sqlx::query_as("SELECT id, label FROM memories WHERE id = ANY($1) ORDER BY id")
        .bind(ids)
        .fetch_all(db)
        .await
}

/// Agents of a workspace by principal, for linking facts back to their cards.
pub async fn agents_by_principal(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    principals: &[String],
) -> Result<Vec<(Uuid, String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, principal_id, name FROM agents
         WHERE workspace_id = $1 AND principal_id = ANY($2) ORDER BY id",
    )
    .bind(workspace_id)
    .bind(principals)
    .fetch_all(db)
    .await
}

/// Agents of a workspace whose name is `name` (case-insensitively).
pub async fn agents_named(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    name: &str,
) -> Result<Vec<(Uuid, String, String)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, principal_id, name FROM agents
         WHERE workspace_id = $1 AND lower(name) = lower($2) ORDER BY id",
    )
    .bind(workspace_id)
    .bind(name)
    .fetch_all(db)
    .await
}

/// Whether a run belongs to the workspace.
pub async fn run_in_workspace(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    run_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM runs r JOIN graphs g ON g.id = r.graph_id
                        WHERE r.id = $1 AND g.workspace_id = $2)",
    )
    .bind(run_id)
    .bind(workspace_id)
    .fetch_one(db)
    .await
}

/// Records the backfill position.
pub async fn advance_backfill(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    span_seq: i64,
    spans: i64,
    records: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE onto_export_cursors
         SET span_seq = GREATEST(span_seq, $2), spans_exported = spans_exported + $3,
             records_exported = records_exported + $4, last_exported_at = now(),
             last_error = NULL, updated_at = now()
         WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .bind(span_seq)
    .bind(spans)
    .bind(records)
    .execute(db)
    .await?;
    Ok(())
}

/// Marks the backfill of a workspace complete.
pub async fn mark_backfilled(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE onto_export_cursors SET backfilled_at = now(), updated_at = now()
         WHERE workspace_id = $1 AND backfilled_at IS NULL",
    )
    .bind(workspace_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Counts an export from the queue.
pub async fn count_exported(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    spans: i64,
    records: i64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE onto_export_cursors
         SET spans_exported = spans_exported + $2, records_exported = records_exported + $3,
             last_exported_at = now(), last_error = NULL, updated_at = now()
         WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .bind(spans)
    .bind(records)
    .execute(db)
    .await?;
    Ok(())
}

/// Notes why the last export of a workspace failed.
pub async fn note_error(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE onto_export_cursors SET last_error = left($2, 500), updated_at = now()
         WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .bind(error)
    .execute(db)
    .await?;
    Ok(())
}
