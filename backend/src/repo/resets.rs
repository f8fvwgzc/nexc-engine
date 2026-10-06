//! One-time password reset links. Only SHA-256 digests of their tokens are
//! stored; a link works once and until it expires, and an account has at
//! most one that still works.

use chrono::{DateTime, Utc};
use sqlx::PgExecutor;
use uuid::Uuid;

/// Stores a link for `user_id` in place of any it had that still worked.
pub async fn issue(
    db: &mut sqlx::PgConnection,
    user_id: Uuid,
    created_by: Uuid,
    token_hash: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM password_resets WHERE user_id = $1 AND used_at IS NULL")
        .bind(user_id)
        .execute(&mut *db)
        .await?;
    sqlx::query(
        "INSERT INTO password_resets (id, user_id, token_hash, created_by, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(token_hash)
    .bind(created_by)
    .bind(expires_at)
    .execute(&mut *db)
    .await?;
    Ok(())
}

/// Uses a link up; returns whose it was, or `None` when it is unknown, was
/// used before, has expired, or belongs to a deleted account. Two requests
/// with the same token cannot both get the account.
pub async fn redeem(
    db: impl PgExecutor<'_>,
    token_hash: &str,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE password_resets r SET used_at = now()
         FROM users u
         WHERE r.token_hash = $1 AND r.used_at IS NULL AND r.expires_at > now()
           AND u.id = r.user_id AND u.deleted_at IS NULL
         RETURNING r.user_id",
    )
    .bind(token_hash)
    .fetch_optional(db)
    .await
}

/// Deletes links that were used or expired more than a day ago.
pub async fn purge_expired(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "DELETE FROM password_resets
         WHERE COALESCE(used_at, expires_at) < now() - interval '1 day'",
    )
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}
