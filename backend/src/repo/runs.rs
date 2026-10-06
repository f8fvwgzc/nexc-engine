//! Runs and per-node results. Runs are claimed by scheduler instances with
//! `SELECT … FOR UPDATE SKIP LOCKED`, so several backend replicas can share
//! one database without executing a run twice.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::graph::{Executor, NodeStatus};
use crate::domain::run::{NodeRun, OUTPUT_PREVIEW_CHARS, Run, RunStatus};

/// A run row without its node results.
#[derive(Debug, Clone)]
pub struct RunRow {
    pub id: Uuid,
    pub graph_id: Uuid,
    pub owner_id: Uuid,
    pub status: RunStatus,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_usd: f64,
    pub max_concurrency: i32,
    pub force: bool,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl RunRow {
    /// Combines the row with its node results.
    pub fn into_run(self, node_runs: Vec<NodeRun>) -> Run {
        Run {
            id: self.id,
            graph_id: self.graph_id,
            status: self.status,
            tokens_in: self.tokens_in,
            tokens_out: self.tokens_out,
            cost_usd: self.cost_usd,
            started_at: self.started_at,
            finished_at: self.finished_at,
            created_at: self.created_at,
            node_runs,
        }
    }
}

impl FromRow<'_, PgRow> for RunRow {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(RunRow {
            id: row.try_get("id")?,
            graph_id: row.try_get("graph_id")?,
            owner_id: row.try_get("owner_id")?,
            status: enum_col(row, "status")?,
            tokens_in: row.try_get("tokens_in")?,
            tokens_out: row.try_get("tokens_out")?,
            cost_usd: row.try_get("cost_usd")?,
            max_concurrency: row.try_get("max_concurrency")?,
            force: row.try_get("force")?,
            started_at: row.try_get("started_at")?,
            finished_at: row.try_get("finished_at")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl FromRow<'_, PgRow> for NodeRun {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(NodeRun {
            node_id: row.try_get("node_id")?,
            status: enum_col(row, "status")?,
            attempt: row.try_get("attempt")?,
            executor: enum_col(row, "executor")?,
            tokens_in: row.try_get("tokens_in")?,
            tokens_out: row.try_get("tokens_out")?,
            cached: row.try_get("cached")?,
            error: row.try_get("error")?,
            started_at: row.try_get("started_at")?,
            finished_at: row.try_get("finished_at")?,
            output_preview: row.try_get("output_preview")?,
        })
    }
}

/// A reusable earlier result with the same content hash.
#[derive(Debug, Clone, FromRow)]
pub struct CachedResult {
    pub run_id: Uuid,
    pub output: String,
    pub tokens_in: i64,
    pub tokens_out: i64,
}

/// Inserts a queued run and one queued node result per `(node, executor)`.
/// Must run inside a transaction.
pub async fn create(
    conn: &mut sqlx::PgConnection,
    graph_id: Uuid,
    owner_id: Uuid,
    max_concurrency: i32,
    force: bool,
    nodes: &[(Uuid, Executor)],
) -> Result<Uuid, sqlx::Error> {
    let run_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO runs (id, graph_id, owner_id, status, max_concurrency, force) VALUES ($1, $2, $3, 'queued', $4, $5)",
    )
    .bind(run_id)
    .bind(graph_id)
    .bind(owner_id)
    .bind(max_concurrency)
    .bind(force)
    .execute(&mut *conn)
    .await?;
    let ids: Vec<Uuid> = nodes.iter().map(|(id, _)| *id).collect();
    let executors: Vec<&str> = nodes.iter().map(|(_, e)| e.as_str()).collect();
    sqlx::query(
        "INSERT INTO node_runs (run_id, node_id, position, status, executor)
         SELECT $1, n.node_id, n.ord::int, 'queued', n.executor
         FROM unnest($2::uuid[], $3::text[]) WITH ORDINALITY AS n(node_id, executor, ord)",
    )
    .bind(run_id)
    .bind(&ids)
    .bind(&executors)
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE nodes SET status = 'queued', updated_at = now() WHERE id = ANY($1)")
        .bind(&ids)
        .execute(&mut *conn)
        .await?;
    Ok(run_id)
}

/// Loads a run row by id (no ownership check; engine use only).
pub async fn find_row(db: impl PgExecutor<'_>, id: Uuid) -> Result<Option<RunRow>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM runs WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Loads a run of a graph that `owner_id` may work on.
pub async fn find_owned(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
    id: Uuid,
) -> Result<Option<RunRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT * FROM runs WHERE id = $1 AND EXISTS (
            SELECT 1 FROM graphs g WHERE g.id = runs.graph_id AND ",
        graph_access!("g", "$2"),
        ")"
    ))
    .bind(id)
    .bind(owner_id)
    .fetch_optional(db)
    .await
}

/// Node results of a run in submission order.
pub async fn node_runs(db: impl PgExecutor<'_>, run_id: Uuid) -> Result<Vec<NodeRun>, sqlx::Error> {
    sqlx::query_as(
        "SELECT node_id, status, attempt, executor, tokens_in, tokens_out, cached, error, started_at, finished_at,
                left(output, $2) AS output_preview
         FROM node_runs WHERE run_id = $1 ORDER BY position",
    )
    .bind(run_id)
    .bind(OUTPUT_PREVIEW_CHARS as i32)
    .fetch_all(db)
    .await
}

/// Runs of a graph, newest first, with their node results.
pub async fn list_for_graph(
    conn: &mut sqlx::PgConnection,
    graph_id: Uuid,
) -> Result<Vec<Run>, sqlx::Error> {
    let rows: Vec<RunRow> =
        sqlx::query_as("SELECT * FROM runs WHERE graph_id = $1 ORDER BY created_at DESC LIMIT 100")
            .bind(graph_id)
            .fetch_all(&mut *conn)
            .await?;
    let mut runs = Vec::with_capacity(rows.len());
    for row in rows {
        let nrs = node_runs(&mut *conn, row.id).await?;
        runs.push(row.into_run(nrs));
    }
    Ok(runs)
}

/// Claims the oldest queued run for `instance`, marking it running.
pub async fn claim_next(
    db: impl PgExecutor<'_>,
    instance: Uuid,
) -> Result<Option<RunRow>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE runs SET status = 'running', claimed_by = $1, started_at = now(), heartbeat_at = now()
         WHERE id = (SELECT id FROM runs WHERE status = 'queued' ORDER BY created_at
                     FOR UPDATE SKIP LOCKED LIMIT 1)
         RETURNING *",
    )
    .bind(instance)
    .fetch_optional(db)
    .await
}

/// Refreshes the heartbeat of every run executed by `instance`.
pub async fn heartbeat(db: impl PgExecutor<'_>, instance: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE runs SET heartbeat_at = now() WHERE claimed_by = $1 AND status = 'running'",
    )
    .bind(instance)
    .execute(db)
    .await?;
    Ok(())
}

/// Fails runs whose executing instance stopped sending heartbeats.
pub async fn fail_orphans(
    db: impl PgExecutor<'_>,
    stale_after_secs: i64,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE runs SET status = 'failed', error = 'interrupted: executing instance stopped', finished_at = now()
         WHERE status = 'running' AND heartbeat_at < now() - make_interval(secs => $1)
         RETURNING id",
    )
    .bind(stale_after_secs as f64)
    .fetch_all(db)
    .await
}

/// Marks unfinished node results of a finished run with `status`.
pub async fn close_unfinished_nodes(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    status: NodeStatus,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE node_runs SET status = $2, finished_at = now()
         WHERE run_id = $1 AND status IN ('queued', 'running') RETURNING node_id",
    )
    .bind(run_id)
    .bind(status.as_str())
    .fetch_all(db)
    .await
}

/// Whether cancellation was requested for a run.
pub async fn cancel_requested(db: impl PgExecutor<'_>, id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT cancel_requested FROM runs WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
}

/// Requests cancellation. A still-queued run is cancelled immediately.
pub async fn request_cancel(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
    id: Uuid,
) -> Result<Option<RunRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        "UPDATE runs SET cancel_requested = true,
                status = CASE WHEN status = 'queued' THEN 'cancelled' ELSE status END,
                finished_at = CASE WHEN status = 'queued' THEN now() ELSE finished_at END
         WHERE id = $1 AND EXISTS (
            SELECT 1 FROM graphs g WHERE g.id = runs.graph_id AND ",
        graph_access!("g", "$2"),
        ") RETURNING *"
    ))
    .bind(id)
    .bind(owner_id)
    .fetch_optional(db)
    .await
}

/// Sets the final status of a run.
pub async fn finish(
    db: impl PgExecutor<'_>,
    id: Uuid,
    status: RunStatus,
) -> Result<RunRow, sqlx::Error> {
    sqlx::query_as("UPDATE runs SET status = $2, finished_at = now() WHERE id = $1 RETURNING *")
        .bind(id)
        .bind(status.as_str())
        .fetch_one(db)
        .await
}

/// Marks a node result as running attempt `attempt`.
pub async fn start_node(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    node_id: Uuid,
    attempt: i32,
    agent_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE node_runs SET status = 'running', attempt = $3, agent_id = $4, error = NULL,
                started_at = COALESCE(started_at, now())
         WHERE run_id = $1 AND node_id = $2",
    )
    .bind(run_id)
    .bind(node_id)
    .bind(attempt)
    .bind(agent_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Final outcome of one node.
#[derive(Debug, Clone)]
pub struct NodeOutcome<'a> {
    pub status: NodeStatus,
    pub attempt: i32,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_usd: f64,
    pub cached: bool,
    pub error: Option<&'a str>,
    pub output: Option<&'a str>,
    pub content_hash: Option<&'a str>,
}

/// Records the outcome of a node and adds its usage to the run totals.
/// Must run inside a transaction.
pub async fn finish_node(
    conn: &mut sqlx::PgConnection,
    run_id: Uuid,
    node_id: Uuid,
    o: &NodeOutcome<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE node_runs SET status = $3, attempt = $4, tokens_in = $5, tokens_out = $6, cost_usd = $7,
                cached = $8, error = $9, output = $10, content_hash = $11, finished_at = now()
         WHERE run_id = $1 AND node_id = $2",
    )
    .bind(run_id)
    .bind(node_id)
    .bind(o.status.as_str())
    .bind(o.attempt)
    .bind(o.tokens_in)
    .bind(o.tokens_out)
    .bind(o.cost_usd)
    .bind(o.cached)
    .bind(o.error)
    .bind(o.output)
    .bind(o.content_hash)
    .execute(&mut *conn)
    .await?;
    if !o.cached && (o.tokens_in > 0 || o.tokens_out > 0) {
        sqlx::query(
            "UPDATE runs SET tokens_in = tokens_in + $2, tokens_out = tokens_out + $3, cost_usd = cost_usd + $4
             WHERE id = $1",
        )
        .bind(run_id)
        .bind(o.tokens_in)
        .bind(o.tokens_out)
        .bind(o.cost_usd)
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// Most recent successful result for `content_hash` within a graph.
pub async fn find_cached(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
    content_hash: &str,
) -> Result<Option<CachedResult>, sqlx::Error> {
    sqlx::query_as(
        "SELECT nr.run_id, nr.output, nr.tokens_in, nr.tokens_out
         FROM node_runs nr JOIN runs r ON r.id = nr.run_id
         WHERE r.graph_id = $1 AND nr.content_hash = $2 AND nr.status = 'succeeded' AND nr.output IS NOT NULL
         ORDER BY nr.finished_at DESC LIMIT 1",
    )
    .bind(graph_id)
    .bind(content_hash)
    .fetch_optional(db)
    .await
}

/// Queue depth, running nodes and active runs for a user's runs.
pub async fn activity(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
) -> Result<(i64, i64, i64), sqlx::Error> {
    let row = sqlx::query(
        "SELECT count(*) FILTER (WHERE nr.status = 'queued') AS queued,
                count(*) FILTER (WHERE nr.status = 'running') AS running,
                count(DISTINCT r.id) AS runs
         FROM runs r LEFT JOIN node_runs nr ON nr.run_id = r.id
         WHERE r.owner_id = $1 AND r.status IN ('queued', 'running')",
    )
    .bind(owner_id)
    .fetch_one(db)
    .await?;
    Ok((
        row.try_get("queued")?,
        row.try_get("running")?,
        row.try_get("runs")?,
    ))
}
