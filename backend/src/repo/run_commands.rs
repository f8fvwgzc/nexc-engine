//! Commands sent into a running run (`run_commands`): recorded by the
//! handler, taken by the instance executing the run.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

/// The one command there is: stop a sub-agent.
pub const CANCEL_AGENT: &str = "cancel_agent";

/// A command as recorded.
#[derive(Debug, Clone)]
pub struct RunCommand {
    pub id: Uuid,
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub command: String,
    pub span_id: Uuid,
    pub subject: String,
    pub reason: String,
    pub requested_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    /// The run's lease epoch when the command was recorded: whose attempt it is about.
    pub lease_epoch: i64,
}

impl FromRow<'_, PgRow> for RunCommand {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(RunCommand {
            id: row.try_get("id")?,
            run_id: row.try_get("run_id")?,
            node_id: row.try_get("node_id")?,
            command: row.try_get("command")?,
            span_id: row.try_get("span_id")?,
            subject: row.try_get("subject")?,
            reason: row.try_get("reason")?,
            requested_by: row.try_get("requested_by")?,
            created_at: row.try_get("created_at")?,
            lease_epoch: row.try_get("lease_epoch")?,
        })
    }
}

/// A command to record.
#[derive(Debug, Clone, Copy)]
pub struct NewCommand<'a> {
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub command: &'a str,
    /// The `spawn` span of the sub-agent the command is about.
    pub span_id: Uuid,
    /// The sub-agent's principal.
    pub subject: &'a str,
    pub reason: &'a str,
    pub requested_by: Uuid,
}

/// Records a command; returns it.
pub async fn insert(
    db: impl PgExecutor<'_>,
    new: NewCommand<'_>,
) -> Result<RunCommand, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO run_commands (id, run_id, node_id, command, span_id, subject, reason, requested_by,
                                   lease_epoch)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, (SELECT lease_epoch FROM runs WHERE id = $2))
         RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(new.run_id)
    .bind(new.node_id)
    .bind(new.command)
    .bind(new.span_id)
    .bind(new.subject)
    .bind(new.reason)
    .bind(new.requested_by)
    .fetch_one(db)
    .await
}

/// Takes every command of the run nobody took yet, marking them taken, for
/// the instance holding the run's lease at `epoch`: a command is addressed
/// to whoever holds the run, so one recorded before a takeover is taken by
/// the new holder, and two instances never deliver the same one.
pub async fn take_pending(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    instance: Uuid,
    epoch: i64,
) -> Result<Vec<RunCommand>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE run_commands SET taken_at = now()
         WHERE id IN (SELECT c.id FROM run_commands c JOIN runs r ON r.id = c.run_id
                      WHERE c.run_id = $1 AND c.taken_at IS NULL
                        AND r.claimed_by = $2 AND r.lease_epoch = $3
                      ORDER BY c.created_at FOR UPDATE OF c SKIP LOCKED)
         RETURNING *",
    )
    .bind(run_id)
    .bind(instance)
    .bind(epoch)
    .fetch_all(db)
    .await
}
