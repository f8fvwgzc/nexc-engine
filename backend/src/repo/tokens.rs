//! Refresh tokens. Only SHA-256 digests are stored; tokens of one login
//! session share a `family_id` so that reuse revokes the whole family.

use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

/// A stored refresh token.
#[derive(Debug, FromRow)]
pub struct RefreshToken {
    pub id: Uuid,
    pub user_id: Uuid,
    pub family_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
}

/// Stores a token digest.
pub async fn insert(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    family_id: Uuid,
    token_hash: &str,
    expires_at: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO refresh_tokens (id, user_id, family_id, token_hash, expires_at) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(family_id)
    .bind(token_hash)
    .bind(expires_at)
    .execute(db)
    .await?;
    Ok(())
}

/// Looks a token up by digest.
pub async fn find(
    db: impl PgExecutor<'_>,
    token_hash: &str,
) -> Result<Option<RefreshToken>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, user_id, family_id, expires_at, used_at, revoked_at FROM refresh_tokens WHERE token_hash = $1",
    )
    .bind(token_hash)
    .fetch_optional(db)
    .await
}

/// Marks a token used. Returns false if it was already used (a concurrent or
/// replayed refresh), which callers treat as reuse.
pub async fn mark_used(db: impl PgExecutor<'_>, id: Uuid) -> Result<bool, sqlx::Error> {
    let done =
        sqlx::query("UPDATE refresh_tokens SET used_at = now() WHERE id = $1 AND used_at IS NULL")
            .bind(id)
            .execute(db)
            .await?;
    Ok(done.rows_affected() == 1)
}

/// Revokes every token of a family.
pub async fn revoke_family(db: impl PgExecutor<'_>, family_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = now() WHERE family_id = $1 AND revoked_at IS NULL",
    )
    .bind(family_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Revokes every token of a user: all their sessions end at the next refresh.
pub async fn revoke_user(db: impl PgExecutor<'_>, user_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE refresh_tokens SET revoked_at = now() WHERE user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user_id)
    .execute(db)
    .await?;
    Ok(())
}

/// How many sessions of a user are alive: sign-ins whose latest token was
/// neither used up, revoked nor expired.
pub async fn active_sessions(db: impl PgExecutor<'_>, user_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(DISTINCT family_id) FROM refresh_tokens
         WHERE user_id = $1 AND revoked_at IS NULL AND used_at IS NULL AND expires_at > now()",
    )
    .bind(user_id)
    .fetch_one(db)
    .await
}

/// Deletes tokens that expired more than a day ago.
pub async fn purge_expired(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    let done =
        sqlx::query("DELETE FROM refresh_tokens WHERE expires_at < now() - interval '1 day'")
            .execute(db)
            .await?;
    Ok(done.rows_affected())
}
