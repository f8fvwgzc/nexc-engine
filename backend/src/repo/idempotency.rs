//! Idempotency keys (`idempotency_keys`): the first response to a write sent
//! with an `Idempotency-Key`, kept for the retries of that write
//! (`http::middleware::idempotency`).

use std::time::Duration;

use sqlx::{PgExecutor, PgPool};
use uuid::Uuid;

/// How long a key is kept.
pub const RETENTION_HOURS: i32 = 24;

/// A response kept for replay.
#[derive(Debug, Clone)]
pub struct Kept {
    pub status: i16,
    pub body: Vec<u8>,
    pub content_type: Option<String>,
}

/// What a key says about a request that carries it.
#[derive(Debug, Clone)]
pub enum Claim {
    /// The key is new: this request is handled, and its response kept.
    First,
    /// The same request was answered already: this is its answer.
    Done(Kept),
    /// The key was used with another request.
    OtherRequest,
    /// The same request is still being handled.
    UnderWay,
}

/// Claims `key` of `user` on `route` for a request whose content hashes to
/// `hash`. A key past its retention, or a first request still unanswered
/// after `stale` (it died with the instance handling it), is free again.
pub async fn claim(
    db: &PgPool,
    user: Uuid,
    route: &str,
    key: &str,
    hash: &[u8; 32],
    stale: Duration,
) -> Result<Claim, sqlx::Error> {
    let mut tx = db.begin().await?;
    sqlx::query(
        "DELETE FROM idempotency_keys
         WHERE user_id = $1 AND route = $2 AND key = $3
           AND (created_at < now() - make_interval(hours => $4)
                OR (response_status IS NULL AND created_at < now() - make_interval(secs => $5)))",
    )
    .bind(user)
    .bind(route)
    .bind(key)
    .bind(RETENTION_HOURS)
    .bind(stale.as_secs_f64())
    .execute(&mut *tx)
    .await?;
    let inserted = sqlx::query(
        "INSERT INTO idempotency_keys (user_id, route, key, request_hash) VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, route, key) DO NOTHING",
    )
    .bind(user)
    .bind(route)
    .bind(key)
    .bind(hash.as_slice())
    .execute(&mut *tx)
    .await?
    .rows_affected()
        == 1;
    if inserted {
        tx.commit().await?;
        return Ok(Claim::First);
    }
    let (kept_hash, status, body, content_type): (
        Vec<u8>,
        Option<i16>,
        Option<Vec<u8>>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT request_hash, response_status, response_body, response_type FROM idempotency_keys
             WHERE user_id = $1 AND route = $2 AND key = $3",
    )
    .bind(user)
    .bind(route)
    .bind(key)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(match status {
        _ if kept_hash != hash.as_slice() => Claim::OtherRequest,
        None => Claim::UnderWay,
        Some(status) => Claim::Done(Kept {
            status,
            body: body.unwrap_or_default(),
            content_type,
        }),
    })
}

/// Keeps the response to the first request with `key`.
pub async fn complete(
    db: impl PgExecutor<'_>,
    user: Uuid,
    route: &str,
    key: &str,
    status: i16,
    body: &[u8],
    content_type: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE idempotency_keys SET response_status = $4, response_body = $5, response_type = $6
         WHERE user_id = $1 AND route = $2 AND key = $3 AND response_status IS NULL",
    )
    .bind(user)
    .bind(route)
    .bind(key)
    .bind(status)
    .bind(body)
    .bind(content_type)
    .execute(db)
    .await?;
    Ok(())
}

/// Frees a key whose first request got no answer worth keeping (a server
/// error, a rate limit): a retry runs the request again.
pub async fn release(
    db: impl PgExecutor<'_>,
    user: Uuid,
    route: &str,
    key: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM idempotency_keys
         WHERE user_id = $1 AND route = $2 AND key = $3 AND response_status IS NULL",
    )
    .bind(user)
    .bind(route)
    .bind(key)
    .execute(db)
    .await?;
    Ok(())
}

/// Deletes the keys past their retention (housekeeping); returns how many.
pub async fn purge_expired(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    Ok(sqlx::query(
        "DELETE FROM idempotency_keys WHERE created_at < now() - make_interval(hours => $1)",
    )
    .bind(RETENTION_HOURS)
    .execute(db)
    .await?
    .rows_affected())
}
