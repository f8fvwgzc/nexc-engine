//! What happened to the lease of each run (`run_lease_events`): claimed,
//! taken over, released, lost and fenced (docs/HA.md).

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

/// The instance claimed a queued run.
pub const CLAIMED: &str = "claimed";
/// The instance took over a run whose lease ran out.
pub const TAKEN_OVER: &str = "taken_over";
/// The holder released its lease (it shut down).
pub const RELEASED: &str = "released";
/// The holder found the epoch moved, or could not renew for a whole lease.
pub const LOST: &str = "lost";
/// A write of a former holder was refused.
pub const FENCED: &str = "fenced";

/// One event of a run's lease.
#[derive(Debug, Clone, FromRow)]
pub struct LeaseEvent {
    pub id: Uuid,
    pub run_id: Uuid,
    pub epoch: i64,
    pub instance: Uuid,
    pub kind: String,
    pub previous: Option<Uuid>,
    pub detail: String,
    pub created_at: DateTime<Utc>,
}

/// An event to record.
#[derive(Debug, Clone, Copy)]
pub struct NewEvent<'a> {
    pub run_id: Uuid,
    pub epoch: i64,
    pub instance: Uuid,
    pub kind: &'a str,
    pub previous: Option<Uuid>,
    pub detail: &'a str,
}

/// Records one event.
pub async fn record(db: impl PgExecutor<'_>, event: NewEvent<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO run_lease_events (id, run_id, epoch, instance, kind, previous, detail)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(Uuid::now_v7())
    .bind(event.run_id)
    .bind(event.epoch)
    .bind(event.instance)
    .bind(event.kind)
    .bind(event.previous)
    .bind(event.detail)
    .execute(db)
    .await?;
    Ok(())
}

/// The events of a run, oldest first.
pub async fn of_run(db: impl PgExecutor<'_>, run_id: Uuid) -> Result<Vec<LeaseEvent>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM run_lease_events WHERE run_id = $1 ORDER BY created_at, id")
        .bind(run_id)
        .fetch_all(db)
        .await
}
