//! Runs and per-node results. Runs are claimed by scheduler instances with
//! `SELECT … FOR UPDATE SKIP LOCKED`, so several backend replicas can share
//! one database without executing a run twice. The instance executing a run
//! holds a lease on it (`claimed_by`, `lease_expires_at`, `lease_epoch`):
//! renewed while it executes, taken over by another instance when it runs
//! out, and checked by every write the executor makes (docs/HA.md).

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::agent::AgentOutputEvidence;
use crate::domain::capabilities::DataLabel;
use crate::domain::graph::{Executor, NodeStatus};
use crate::domain::policy::PolicyVersions;
use crate::domain::run::{NodeRun, OUTPUT_PREVIEW_CHARS, Run, RunStatus};

/// Reads a `jsonb` column holding [`PolicyVersions`]; anything odd reads as none.
fn versions_col(row: &PgRow, col: &str) -> Result<PolicyVersions, sqlx::Error> {
    let raw: serde_json::Value = row.try_get(col)?;
    Ok(serde_json::from_value(raw).unwrap_or_default())
}

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
    /// The final hash of the run's trace chain, once sealed.
    pub trace_hash: Option<Vec<u8>>,
    pub policy_mismatches: i32,
    pub policy_versions: PolicyVersions,
    /// The instance holding the run's lease (the one executing it).
    pub claimed_by: Option<Uuid>,
    /// Raised every time the run changes hands; writes are fenced on it.
    pub lease_epoch: i64,
    /// Someone asked for the run to stop (`POST /runs/{id}/cancel`).
    pub cancel_requested: bool,
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
            trace_hash: self.trace_hash.as_deref().map(hex::encode),
            policy_mismatches: self.policy_mismatches,
            policy_versions: self.policy_versions,
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
            trace_hash: row.try_get("trace_hash")?,
            policy_mismatches: row.try_get("policy_mismatches")?,
            policy_versions: versions_col(row, "policy_versions")?,
            claimed_by: row.try_get("claimed_by")?,
            lease_epoch: row.try_get("lease_epoch")?,
            cancel_requested: row.try_get("cancel_requested")?,
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
            policy_versions: versions_col(row, "policy_versions")?,
            context_labels: row
                .try_get::<Vec<String>, _>("context_labels")?
                .iter()
                .filter_map(|l| l.parse().ok())
                .collect(),
            evidence: row
                .try_get::<Option<serde_json::Value>, _>("evidence")?
                .and_then(|v| serde_json::from_value(v).ok()),
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
    /// The verdict the output got when it was produced.
    pub evidence: Option<serde_json::Value>,
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
                left(output, $2) AS output_preview, policy_versions, context_labels, evidence
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

/// Claims the oldest queued run for `instance`, marking it running under a
/// lease of `lease_secs` at the next epoch.
pub async fn claim_next(
    db: impl PgExecutor<'_>,
    instance: Uuid,
    lease_secs: f64,
) -> Result<Option<RunRow>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE runs SET status = 'running', claimed_by = $1, started_at = now(), heartbeat_at = now(),
                lease_expires_at = now() + make_interval(secs => $2), lease_epoch = lease_epoch + 1
         WHERE id = (SELECT id FROM runs WHERE status = 'queued' ORDER BY created_at
                     FOR UPDATE SKIP LOCKED LIMIT 1)
         RETURNING *",
    )
    .bind(instance)
    .bind(lease_secs)
    .fetch_optional(db)
    .await
}

/// Claims one queued run for `instance` by id (a run this instance will
/// execute itself rather than leave to a dispatcher), marking it running
/// under a lease of `lease_secs` at the next epoch.
pub async fn claim(
    db: impl PgExecutor<'_>,
    instance: Uuid,
    run_id: Uuid,
    lease_secs: f64,
) -> Result<Option<RunRow>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE runs SET status = 'running', claimed_by = $1, started_at = now(), heartbeat_at = now(),
                lease_expires_at = now() + make_interval(secs => $3), lease_epoch = lease_epoch + 1
         WHERE id = $2 AND status = 'queued' RETURNING *",
    )
    .bind(instance)
    .bind(run_id)
    .bind(lease_secs)
    .fetch_optional(db)
    .await
}

/// A run taken over from a holder whose lease ran out.
#[derive(Debug, Clone)]
pub struct Takeover {
    /// The run as it is now: held by the instance that took it, at the next epoch.
    pub row: RunRow,
    /// The instance that held it before.
    pub previous: Option<Uuid>,
    /// Whether that instance released it (it shut down) rather than stopped renewing.
    pub released: bool,
}

impl FromRow<'_, PgRow> for Takeover {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Takeover {
            row: RunRow::from_row(row)?,
            previous: row.try_get("previous_owner")?,
            released: row.try_get("released")?,
        })
    }
}

/// Takes over the running run whose lease ran out longest ago, for
/// `instance`, in one statement that raises its epoch: from then on the
/// former holder's writes are refused. Runs in `local` (executing here) are
/// left alone. A holder that still sends heartbeats (an instance of an
/// earlier version, which knows no leases) is never taken over; a run such
/// an instance claimed has no lease at all, and is taken over once its
/// heartbeat is older than a lease. An external agent's session (claimed
/// by nobody) is never taken over.
pub async fn take_over(
    db: impl PgExecutor<'_>,
    instance: Uuid,
    lease_secs: f64,
    local: &[Uuid],
) -> Result<Option<Takeover>, sqlx::Error> {
    sqlx::query_as(
        "WITH expired AS (
             SELECT id, claimed_by, heartbeat_at IS NULL AS released FROM runs
             WHERE status = 'running'
               AND (lease_expires_at < now() OR (lease_expires_at IS NULL AND claimed_by IS NOT NULL))
               AND (heartbeat_at IS NULL OR heartbeat_at < now() - make_interval(secs => $2))
               AND NOT (id = ANY($3))
             ORDER BY lease_expires_at FOR UPDATE SKIP LOCKED LIMIT 1)
         UPDATE runs r SET claimed_by = $1, heartbeat_at = now(),
                lease_expires_at = now() + make_interval(secs => $2), lease_epoch = r.lease_epoch + 1
         FROM expired WHERE r.id = expired.id
         RETURNING r.*, expired.claimed_by AS previous_owner, expired.released",
    )
    .bind(instance)
    .bind(lease_secs)
    .bind(local)
    .fetch_optional(db)
    .await
}

/// Renews the leases `instance` holds, each at its epoch (`(run, epoch)`);
/// returns the runs renewed. A run left out finished, or changed hands.
pub async fn renew(
    db: impl PgExecutor<'_>,
    instance: Uuid,
    held: &[(Uuid, i64)],
    lease_secs: f64,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let ids: Vec<Uuid> = held.iter().map(|(id, _)| *id).collect();
    let epochs: Vec<i64> = held.iter().map(|(_, epoch)| *epoch).collect();
    sqlx::query_scalar(
        "UPDATE runs r SET heartbeat_at = now(), lease_expires_at = now() + make_interval(secs => $4)
         FROM unnest($1::uuid[], $2::bigint[]) AS l(id, epoch)
         WHERE r.id = l.id AND r.lease_epoch = l.epoch AND r.claimed_by = $3 AND r.status = 'running'
         RETURNING r.id",
    )
    .bind(&ids)
    .bind(&epochs)
    .bind(instance)
    .bind(lease_secs)
    .fetch_all(db)
    .await
}

/// Releases the lease held at `epoch` (the holder shuts down): the lease runs
/// out now and the epoch goes up, so a peer takes the run over at once and
/// nothing the holder still sends is accepted. Returns the new epoch.
pub async fn release(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    epoch: i64,
) -> Result<Option<i64>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE runs SET lease_epoch = lease_epoch + 1, lease_expires_at = now(), heartbeat_at = NULL
         WHERE id = $1 AND lease_epoch = $2 AND status = 'running' RETURNING lease_epoch",
    )
    .bind(run_id)
    .bind(epoch)
    .fetch_optional(db)
    .await
}

/// Locks the row of a running run for a write of its executor and returns
/// its epoch (`None` when the run is no longer running). The lock is held to
/// the end of the transaction, so a takeover waits for the write, and a
/// write after a takeover sees the new epoch. A write that changes the run
/// row itself takes `FOR NO KEY UPDATE` (`updates_run`); the others share
/// the row (`FOR SHARE`), so they do not wait for each other.
///
/// The transaction may then sit idle for at most `idle_limit` between its
/// statements (`idle_in_transaction_session_timeout`, for this transaction
/// only): when the instance writing vanishes in the middle (its host died,
/// the network between it and the database broke), the database ends its
/// session after that instead of keeping the row locked until TCP notices,
/// which would hold off every takeover of the run.
pub async fn lock_lease(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    updates_run: bool,
    idle_limit: std::time::Duration,
) -> Result<Option<i64>, sqlx::Error> {
    let sql = if updates_run {
        "SELECT lease_epoch, set_config('idle_in_transaction_session_timeout', $2, true)
         FROM runs WHERE id = $1 AND status = 'running' FOR NO KEY UPDATE"
    } else {
        "SELECT lease_epoch, set_config('idle_in_transaction_session_timeout', $2, true)
         FROM runs WHERE id = $1 AND status = 'running' FOR SHARE"
    };
    sqlx::query_scalar(sql)
        .bind(run_id)
        .bind(idle_limit.as_millis().max(1).to_string())
        .fetch_optional(db)
        .await
}

/// What the run's cancel watcher looks at every second: whether a cancel was
/// asked for, and who holds the run at which epoch.
#[derive(Debug, Clone, FromRow)]
pub struct Watched {
    pub cancel_requested: bool,
    pub claimed_by: Option<Uuid>,
    pub lease_epoch: i64,
}

/// The run as its cancel watcher sees it.
pub async fn watched(db: impl PgExecutor<'_>, id: Uuid) -> Result<Option<Watched>, sqlx::Error> {
    sqlx::query_as("SELECT cancel_requested, claimed_by, lease_epoch FROM runs WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// A node result as stored, for resuming a run that was taken over.
#[derive(Debug, Clone)]
pub struct StoredNode {
    pub node_id: Uuid,
    pub status: NodeStatus,
    pub attempt: i32,
    pub agent_id: Option<Uuid>,
    pub output: Option<String>,
    /// Attempts cut short by a hand-off on shutdown, which do not count
    /// against the node's attempts.
    pub handoffs: i32,
}

impl FromRow<'_, PgRow> for StoredNode {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(StoredNode {
            node_id: row.try_get("node_id")?,
            status: enum_col(row, "status")?,
            attempt: row.try_get("attempt")?,
            agent_id: row.try_get("agent_id")?,
            output: row.try_get("output")?,
            handoffs: row.try_get("handoffs")?,
        })
    }
}

/// Every node result of a run, with its full output.
pub async fn stored_nodes(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
) -> Result<Vec<StoredNode>, sqlx::Error> {
    sqlx::query_as(
        "SELECT node_id, status, attempt, agent_id, output, handoffs FROM node_runs
         WHERE run_id = $1 ORDER BY position",
    )
    .bind(run_id)
    .fetch_all(db)
    .await
}

/// Counts a hand-off against every node of a run whose attempt it cut short
/// (running, or waiting for an approval): the run's holder shut down and
/// released it. An attempt is counted once, however many hand-offs it waited
/// through. Returns how many nodes it counted.
pub async fn count_handoffs(
    conn: &mut sqlx::PgConnection,
    run_id: Uuid,
) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE node_runs SET handoffs = handoffs + 1, handed_off_attempt = attempt
         WHERE run_id = $1 AND status IN ('running', 'waiting_approval')
           AND attempt > handed_off_attempt",
    )
    .bind(run_id)
    .execute(conn)
    .await?;
    Ok(done.rows_affected())
}

/// Queues a node again whose attempt was interrupted (the node run and the
/// node itself); its attempt counter is kept, the next attempt is one more.
pub async fn requeue_node(
    conn: &mut sqlx::PgConnection,
    run_id: Uuid,
    node_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE node_runs SET status = 'queued'
         WHERE run_id = $1 AND node_id = $2 AND status IN ('running', 'waiting_approval')",
    )
    .bind(run_id)
    .bind(node_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query("UPDATE nodes SET status = 'queued', updated_at = now() WHERE id = $1")
        .bind(node_id)
        .execute(&mut *conn)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// The status of one node of a run.
pub async fn node_status(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    node_id: Uuid,
) -> Result<Option<NodeStatus>, sqlx::Error> {
    let status: Option<String> =
        sqlx::query_scalar("SELECT status FROM node_runs WHERE run_id = $1 AND node_id = $2")
            .bind(run_id)
            .bind(node_id)
            .fetch_optional(db)
            .await?;
    Ok(status.and_then(|s| s.parse().ok()))
}

/// Marks unfinished node results of a finished run with `status`.
pub async fn close_unfinished_nodes(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    status: NodeStatus,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE node_runs SET status = $2, finished_at = now()
         WHERE run_id = $1 AND status IN ('queued', 'running', 'waiting_approval') RETURNING node_id",
    )
    .bind(run_id)
    .bind(status.as_str())
    .fetch_all(db)
    .await
}

/// Records why a node of a run ended (a node closed before it was started).
pub async fn set_node_error(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    node_id: Uuid,
    error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE node_runs SET error = $3 WHERE run_id = $1 AND node_id = $2")
        .bind(run_id)
        .bind(node_id)
        .bind(error)
        .execute(db)
        .await?;
    Ok(())
}

/// Counts one more action of the run that its policy did not allow.
pub async fn add_policy_mismatch(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE runs SET policy_mismatches = policy_mismatches + 1 WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Agents that were quarantined while a node of the run was running on them.
pub async fn quarantined_agents_running(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT DISTINCT a.id FROM node_runs nr JOIN agents a ON a.id = nr.agent_id
         WHERE nr.run_id = $1 AND nr.status IN ('running', 'waiting_approval') AND a.status = 'quarantined'",
    )
    .bind(run_id)
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

/// Sets the final status of a run and seals its trace with `trace_hash`.
pub async fn finish(
    db: impl PgExecutor<'_>,
    id: Uuid,
    status: RunStatus,
    trace_hash: Option<&[u8]>,
) -> Result<RunRow, sqlx::Error> {
    sqlx::query_as(
        "UPDATE runs SET status = $2, finished_at = now(), trace_hash = $3 WHERE id = $1 RETURNING *",
    )
    .bind(id)
    .bind(status.as_str())
    .bind(trace_hash)
    .fetch_one(db)
    .await
}

/// Marks a node result as running attempt `attempt`, under `policy_versions`,
/// having been given data of `labels`.
pub async fn start_node(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    node_id: Uuid,
    attempt: i32,
    agent_id: Option<Uuid>,
    policy_versions: &PolicyVersions,
    labels: &[DataLabel],
) -> Result<(), sqlx::Error> {
    let labels: Vec<&str> = labels.iter().map(|l| l.as_str()).collect();
    sqlx::query(
        "UPDATE node_runs SET status = 'running', attempt = $3, agent_id = $4, error = NULL,
                started_at = COALESCE(started_at, now()), policy_versions = $5, context_labels = $6
         WHERE run_id = $1 AND node_id = $2",
    )
    .bind(run_id)
    .bind(node_id)
    .bind(attempt)
    .bind(agent_id)
    .bind(serde_json::to_value(policy_versions).expect("versions serialise"))
    .bind(&labels)
    .execute(db)
    .await?;
    Ok(())
}

/// Pauses a running node for an approval, or resumes it: the node run and
/// the node itself change status between `running` and `waiting_approval`.
pub async fn set_waiting(
    db: &mut sqlx::PgConnection,
    run_id: Uuid,
    node_id: Uuid,
    waiting: bool,
) -> Result<bool, sqlx::Error> {
    let (from, to) = if waiting {
        ("running", "waiting_approval")
    } else {
        ("waiting_approval", "running")
    };
    let done = sqlx::query(
        "UPDATE node_runs SET status = $4 WHERE run_id = $1 AND node_id = $2 AND status = $3",
    )
    .bind(run_id)
    .bind(node_id)
    .bind(from)
    .bind(to)
    .execute(&mut *db)
    .await?;
    if done.rows_affected() == 1 {
        sqlx::query(
            "UPDATE nodes SET status = $2, updated_at = now() WHERE id = $1 AND status = $3",
        )
        .bind(node_id)
        .bind(to)
        .bind(from)
        .execute(&mut *db)
        .await?;
    }
    Ok(done.rows_affected() == 1)
}

/// Records the workspace policy version that governs a run.
pub async fn set_policy_versions(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    versions: &PolicyVersions,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE runs SET policy_versions = $2 WHERE id = $1")
        .bind(run_id)
        .bind(serde_json::to_value(versions).expect("versions serialise"))
        .execute(db)
        .await?;
    Ok(())
}

/// The classifications of the data the given nodes were given in any
/// successful run: what their outputs are derived from.
pub async fn context_labels_of(
    db: impl PgExecutor<'_>,
    node_ids: &[Uuid],
) -> Result<Vec<DataLabel>, sqlx::Error> {
    if node_ids.is_empty() {
        return Ok(Vec::new());
    }
    let labels: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT l FROM node_runs, unnest(context_labels) AS l
         WHERE node_id = ANY($1) AND status = 'succeeded'",
    )
    .bind(node_ids)
    .fetch_all(db)
    .await?;
    Ok(labels.iter().filter_map(|l| l.parse().ok()).collect())
}

/// The verdicts on an agent's last `limit` outputs across the graphs `user`
/// may open, newest first.
pub async fn evidence_of_agent(
    db: impl PgExecutor<'_>,
    user: Uuid,
    agent_id: Uuid,
    limit: i64,
) -> Result<Vec<AgentOutputEvidence>, sqlx::Error> {
    let rows = sqlx::query(concat!(
        "SELECT nr.run_id, g.id AS graph_id, nr.node_id, nr.evidence->>'verdict' AS verdict,
                (nr.evidence->>'coverage')::float8 AS coverage, nr.finished_at
         FROM node_runs nr JOIN runs r ON r.id = nr.run_id JOIN graphs g ON g.id = r.graph_id
         WHERE nr.agent_id = $1 AND nr.evidence IS NOT NULL AND ",
        graph_access!("g", "$2"),
        " ORDER BY nr.finished_at DESC NULLS LAST, nr.run_id DESC LIMIT $3"
    ))
    .bind(agent_id)
    .bind(user)
    .bind(limit)
    .fetch_all(db)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let verdict = row.try_get::<String, _>("verdict").ok()?.parse().ok()?;
            Some(AgentOutputEvidence {
                run_id: row.try_get("run_id").ok()?,
                graph_id: row.try_get("graph_id").ok()?,
                node_id: row.try_get("node_id").ok()?,
                verdict,
                coverage: row
                    .try_get::<Option<f64>, _>("coverage")
                    .ok()?
                    .unwrap_or(0.0),
                finished_at: row.try_get("finished_at").ok()?,
            })
        })
        .collect())
}

/// The classifications of the data one node of a run was given.
pub async fn context_labels(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    node_id: Uuid,
) -> Result<Vec<DataLabel>, sqlx::Error> {
    let labels: Option<Vec<String>> = sqlx::query_scalar(
        "SELECT context_labels FROM node_runs WHERE run_id = $1 AND node_id = $2",
    )
    .bind(run_id)
    .bind(node_id)
    .fetch_optional(db)
    .await?;
    Ok(labels
        .unwrap_or_default()
        .iter()
        .filter_map(|l| l.parse().ok())
        .collect())
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
    /// What the verifier found about the output (`Evidence` as JSON), when it answered.
    pub evidence: Option<serde_json::Value>,
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
                cached = $8, error = $9, output = $10, content_hash = $11, evidence = $12, finished_at = now()
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
    .bind(&o.evidence)
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
        "SELECT nr.run_id, nr.output, nr.tokens_in, nr.tokens_out, nr.evidence
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
                count(*) FILTER (WHERE nr.status IN ('running', 'waiting_approval')) AS running,
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
