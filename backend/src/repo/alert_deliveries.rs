//! The outbox of alert routing (`alert_deliveries`): filled when something
//! happens, taken by the worker with `SKIP LOCKED`, kept as the log.

use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::alert_routing::{AlertDelivery, DeliveryEvent, DeliveryStatus};

impl FromRow<'_, PgRow> for AlertDelivery {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let status: DeliveryStatus = enum_col(row, "status")?;
        let next_attempt_at: DateTime<Utc> = row.try_get("next_attempt_at")?;
        Ok(AlertDelivery {
            id: row.try_get("id")?,
            destination_id: row.try_get("destination_id")?,
            event: enum_col(row, "event")?,
            alert_id: row.try_get("alert_id")?,
            approval_id: row.try_get("approval_id")?,
            status,
            attempts: row.try_get("attempts")?,
            next_attempt_at: (status == DeliveryStatus::Pending).then_some(next_attempt_at),
            last_attempt_at: row.try_get("last_attempt_at")?,
            last_status: row.try_get("last_status")?,
            last_error: row.try_get("last_error")?,
            created_at: row.try_get("created_at")?,
            delivered_at: row.try_get("delivered_at")?,
        })
    }
}

/// A delivery the worker took, with what to send.
#[derive(Debug, Clone)]
pub struct Claimed {
    pub delivery: AlertDelivery,
    pub workspace_id: Uuid,
    pub payload: Value,
}

impl FromRow<'_, PgRow> for Claimed {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Claimed {
            delivery: AlertDelivery::from_row(row)?,
            workspace_id: row.try_get("workspace_id")?,
            payload: row.try_get("payload")?,
        })
    }
}

/// What goes into the outbox.
#[derive(Debug, Clone)]
pub struct NewDelivery<'a> {
    pub id: Uuid,
    pub destination_id: Uuid,
    pub workspace_id: Uuid,
    pub event: DeliveryEvent,
    pub alert_id: Option<Uuid>,
    pub approval_id: Option<Uuid>,
    pub payload: &'a Value,
}

/// Puts a delivery in the outbox, due at once.
pub async fn enqueue(db: impl PgExecutor<'_>, new: &NewDelivery<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO alert_deliveries (id, destination_id, workspace_id, event, alert_id, approval_id, payload)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(new.id)
    .bind(new.destination_id)
    .bind(new.workspace_id)
    .bind(new.event.as_str())
    .bind(new.alert_id)
    .bind(new.approval_id)
    .bind(new.payload)
    .execute(db)
    .await?;
    Ok(())
}

/// Records a delivery that was sent at once (`POST …/test`), with its outcome.
pub async fn insert_settled(
    db: impl PgExecutor<'_>,
    new: &NewDelivery<'_>,
    delivered: bool,
    status: Option<i32>,
    error: &str,
) -> Result<AlertDelivery, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO alert_deliveries (id, destination_id, workspace_id, event, alert_id, approval_id, payload,
                                       status, attempts, last_attempt_at, last_status, last_error, delivered_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1, now(), $9, $10,
                 CASE WHEN $8 = 'delivered' THEN now() END)
         RETURNING *",
    )
    .bind(new.id)
    .bind(new.destination_id)
    .bind(new.workspace_id)
    .bind(new.event.as_str())
    .bind(new.alert_id)
    .bind(new.approval_id)
    .bind(new.payload)
    .bind(if delivered { "delivered" } else { "failed" })
    .bind(status)
    .bind(error)
    .fetch_one(db)
    .await
}

/// Takes up to `limit` due deliveries for this pass: each is leased for
/// `lease_secs` (so an instance that dies mid-pass loses nothing for long)
/// and counted as one more attempt. Rows another instance holds are skipped.
pub async fn claim(
    db: impl PgExecutor<'_>,
    limit: i64,
    lease_secs: i64,
) -> Result<Vec<Claimed>, sqlx::Error> {
    sqlx::query_as(
        "WITH due AS (
            SELECT id FROM alert_deliveries
            WHERE status = 'pending' AND next_attempt_at <= now()
            ORDER BY next_attempt_at, id LIMIT $1
            FOR UPDATE SKIP LOCKED
         )
         UPDATE alert_deliveries d
         SET attempts = d.attempts + 1, last_attempt_at = now(),
             next_attempt_at = now() + make_interval(secs => $2)
         FROM due WHERE d.id = due.id
         RETURNING d.*",
    )
    .bind(limit)
    .bind(lease_secs as f64)
    .fetch_all(db)
    .await
}

/// The delivery went through.
pub async fn mark_delivered(
    db: impl PgExecutor<'_>,
    id: Uuid,
    status: Option<i32>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE alert_deliveries SET status = 'delivered', delivered_at = now(), last_status = $2,
                last_error = '' WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .execute(db)
    .await?;
    Ok(())
}

/// The delivery failed and is tried again at `next_attempt_at`.
pub async fn mark_retry(
    db: impl PgExecutor<'_>,
    id: Uuid,
    next_attempt_at: DateTime<Utc>,
    status: Option<i32>,
    error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE alert_deliveries SET next_attempt_at = $2, last_status = $3, last_error = $4
         WHERE id = $1",
    )
    .bind(id)
    .bind(next_attempt_at)
    .bind(status)
    .bind(error)
    .execute(db)
    .await?;
    Ok(())
}

/// The delivery is given up.
pub async fn mark_failed(
    db: impl PgExecutor<'_>,
    id: Uuid,
    status: Option<i32>,
    error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE alert_deliveries SET status = 'failed', last_status = $2, last_error = $3
         WHERE id = $1",
    )
    .bind(id)
    .bind(status)
    .bind(error)
    .execute(db)
    .await?;
    Ok(())
}

/// The log of a destination, newest first.
pub async fn log(
    db: impl PgExecutor<'_>,
    destination_id: Uuid,
    limit: i64,
) -> Result<Vec<AlertDelivery>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM alert_deliveries WHERE destination_id = $1
         ORDER BY created_at DESC, id DESC LIMIT $2",
    )
    .bind(destination_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Deliveries still to go out (the worker's backlog), for the operator.
pub async fn pending(db: impl PgExecutor<'_>) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM alert_deliveries WHERE status = 'pending'")
        .fetch_one(db)
        .await
}
