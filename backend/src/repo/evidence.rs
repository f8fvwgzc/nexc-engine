//! The only ways evidence leaves the database. `agent_spans` and `audit_log`
//! refuse updates and deletes (migration `20261020000203`) unless the
//! transaction first says it is a purge path: the retention purge, the
//! erasure of a whole workspace, or an erased account's graphs from before
//! workspaces existed. Erasing an account's name from the audit log is the
//! one update allowed, and says so too.

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgConnection, PgExecutor, PgPool};
use uuid::Uuid;

use crate::domain::audit::AuditAction;

/// Lets this transaction delete spans and audit entries: the setting names
/// the transaction's own id, so it opens nothing anywhere else, and ends with it.
pub async fn allow_purge(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('nexc.evidence_purge', pg_current_xact_id()::text, true)")
        .execute(conn)
        .await?;
    Ok(())
}

/// Lets this transaction replace names in audit entries (dropping their salt).
pub async fn allow_redact(conn: &mut PgConnection) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('nexc.evidence_redact', pg_current_xact_id()::text, true)")
        .execute(conn)
        .await?;
    Ok(())
}

/// The retention of a workspace's recordings in days, as SQL over the
/// guardrails row aliased `wg` (30 when the workspace set none).
macro_rules! retention_days {
    () => {
        "COALESCE((wg.config->'recording'->>'retention_days')::int, 30)"
    };
}

/// How long a deleted graph's evidence is kept, as SQL over `wg`: the
/// workspace's retention, never less than [`EVIDENCE_DAYS_MIN`] days, so that
/// shortening the retention and then deleting a graph does not empty it at once.
macro_rules! evidence_days {
    () => {
        "GREATEST(COALESCE((wg.config->'recording'->>'retention_days')::int, 30), 30)"
    };
}

/// The fewest days a deleted graph's evidence is kept (`evidence_days!`).
pub const EVIDENCE_DAYS_MIN: i64 = 30;

/// Recorded payloads deleted by one sweep, per workspace.
#[derive(Debug, Clone, FromRow)]
pub struct PurgedPayloads {
    pub workspace_id: Option<Uuid>,
    pub retention_days: i32,
    pub count: i64,
}

/// The retention purge of recorded payloads: deletes every payload older
/// than its workspace's retention, a day at a time (a payload goes on the
/// first sweep of the day after its retention ends, so each workspace gets
/// one entry a day, whichever replica sweeps first), and writes an
/// `evidence_purged` entry to each workspace's audit log saying how many
/// went, in one transaction. Returns how many went in all.
pub async fn purge_payloads(pool: &PgPool) -> Result<u64, sqlx::Error> {
    let mut tx = pool.begin().await?;
    let purged = delete_expired_payloads(&mut tx).await?;
    for p in &purged {
        let Some(workspace) = p.workspace_id else {
            continue;
        };
        let detail = format!(
            "{} recorded payload(s) older than {} day(s) deleted by the retention purge",
            p.count, p.retention_days
        );
        super::audit::record_system(
            &mut *tx,
            workspace,
            AuditAction::EvidencePurged,
            "recorded payloads",
            &detail,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(purged.iter().map(|p| p.count.max(0) as u64).sum())
}

async fn delete_expired_payloads(
    conn: &mut PgConnection,
) -> Result<Vec<PurgedPayloads>, sqlx::Error> {
    sqlx::query_as(concat!(
        "WITH gone AS (
             DELETE FROM span_payloads p
             USING runs r
             JOIN graphs g ON g.id = r.graph_id
             LEFT JOIN workspace_guardrails wg ON wg.workspace_id = g.workspace_id
             WHERE p.run_id = r.id
               AND p.created_at < date_trunc('day', now()) - make_interval(days => ",
        retention_days!(),
        ")
             RETURNING g.workspace_id, ",
        retention_days!(),
        " AS retention_days)
         SELECT workspace_id, retention_days, count(*) AS count FROM gone
         GROUP BY workspace_id, retention_days"
    ))
    .fetch_all(conn)
    .await
}

/// A deleted graph whose runs' evidence is past its workspace's retention.
#[derive(Debug, Clone, FromRow)]
pub struct ExpiredGraph {
    pub id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub name: String,
    pub deleted_at: DateTime<Utc>,
    pub retention_days: i32,
}

/// Deleted graphs whose evidence may go: deleted more than `evidence_days!`
/// ago, with no run that ended, or showed any sign of life, within that time
/// (an external run left `running` by a lost session does not hold it forever).
/// Oldest first, at most `limit`.
pub async fn expired_graphs(
    db: impl PgExecutor<'_>,
    limit: i64,
) -> Result<Vec<ExpiredGraph>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT g.id, g.workspace_id, g.name, g.deleted_at, ",
        evidence_days!(),
        " AS retention_days
         FROM graphs g
         LEFT JOIN workspace_guardrails wg ON wg.workspace_id = g.workspace_id
         WHERE g.deleted_at IS NOT NULL
           AND g.deleted_at < now() - make_interval(days => ",
        evidence_days!(),
        ")
           AND NOT EXISTS (
               SELECT 1 FROM runs r WHERE r.graph_id = g.id
                 AND COALESCE(r.finished_at, GREATEST(r.heartbeat_at, r.started_at, r.created_at))
                     >= now() - make_interval(days => ",
        evidence_days!(),
        "))
         ORDER BY g.deleted_at LIMIT $1"
    ))
    .bind(limit)
    .fetch_all(db)
    .await
}

/// What removing a graph for good took with it.
#[derive(Debug, Clone, Default, FromRow)]
pub struct Removed {
    pub runs: i64,
    pub spans: i64,
    pub payloads: i64,
}

/// What a graph's runs hold.
pub async fn graph_contents(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
) -> Result<Removed, sqlx::Error> {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM runs WHERE graph_id = $1) AS runs,
                (SELECT count(*) FROM agent_spans s JOIN runs r ON r.id = s.run_id
                  WHERE r.graph_id = $1) AS spans,
                (SELECT count(*) FROM span_payloads p JOIN runs r ON r.id = p.run_id
                  WHERE r.graph_id = $1) AS payloads",
    )
    .bind(graph_id)
    .fetch_one(db)
    .await
}

/// Removes a deleted graph for good, with its runs and their evidence. The
/// transaction must have called [`allow_purge`]. Returns whether it went.
pub async fn remove_graph(conn: &mut PgConnection, graph_id: Uuid) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM graphs WHERE id = $1 AND deleted_at IS NOT NULL")
        .bind(graph_id)
        .execute(conn)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// What a whole workspace holds as evidence, said before it is erased.
#[derive(Debug, Clone, Default, FromRow)]
pub struct WorkspaceEvidence {
    pub runs: i64,
    pub spans: i64,
    pub audit_entries: i64,
    /// The audit chain's last position and hash, when it has entries.
    pub head_seq: Option<i64>,
    pub head_hash: Option<Vec<u8>>,
}

impl WorkspaceEvidence {
    /// In words, for the platform log.
    pub fn describe(&self) -> String {
        let head = match (self.head_seq, &self.head_hash) {
            (Some(seq), Some(hash)) => format!(
                "; audit chain ended at entry {seq}, sha256 {}",
                hex::encode(hash)
            ),
            _ => String::new(),
        };
        format!(
            "evidence erased: {} run(s), {} span(s), {} audit entr{}{head}",
            self.runs,
            self.spans,
            self.audit_entries,
            if self.audit_entries == 1 { "y" } else { "ies" }
        )
    }
}

/// Counts what a workspace holds as evidence.
pub async fn of_workspace(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<WorkspaceEvidence, sqlx::Error> {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM runs r JOIN graphs g ON g.id = r.graph_id
                  WHERE g.workspace_id = $1) AS runs,
                (SELECT count(*) FROM agent_spans s JOIN runs r ON r.id = s.run_id
                  JOIN graphs g ON g.id = r.graph_id WHERE g.workspace_id = $1) AS spans,
                (SELECT count(*) FROM audit_log WHERE workspace_id = $1) AS audit_entries,
                (SELECT seq FROM audit_chain_heads WHERE workspace_id = $1 AND seq >= 0) AS head_seq,
                (SELECT hash FROM audit_chain_heads WHERE workspace_id = $1 AND seq >= 0) AS head_hash",
    )
    .bind(workspace_id)
    .fetch_one(db)
    .await
}
