//! Cross-instance realtime fan-out over PostgreSQL `LISTEN` / `NOTIFY`.
//! Payloads larger than the NOTIFY limit are parked in `realtime_outbox`
//! and sent by reference.

use sqlx::PgExecutor;

/// The notification channel shared by all instances.
pub const CHANNEL: &str = "nexc_events";

/// Largest payload sent inline (PostgreSQL's limit is 8000 bytes).
pub const INLINE_LIMIT: usize = 7_500;

/// Sends `payload` on [`CHANNEL`].
pub async fn notify(db: impl PgExecutor<'_>, payload: &str) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_notify($1, $2)")
        .bind(CHANNEL)
        .bind(payload)
        .execute(db)
        .await?;
    Ok(())
}

/// Stores a large payload; returns its id.
pub async fn park(
    db: impl PgExecutor<'_>,
    payload: &serde_json::Value,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("INSERT INTO realtime_outbox (payload) VALUES ($1) RETURNING id")
        .bind(payload)
        .fetch_one(db)
        .await
}

/// Loads a parked payload.
pub async fn fetch(
    db: impl PgExecutor<'_>,
    id: i64,
) -> Result<Option<serde_json::Value>, sqlx::Error> {
    sqlx::query_scalar("SELECT payload FROM realtime_outbox WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Deletes parked payloads older than five minutes.
pub async fn purge(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    let done =
        sqlx::query("DELETE FROM realtime_outbox WHERE created_at < now() - interval '5 minutes'")
            .execute(db)
            .await?;
    Ok(done.rows_affected())
}
