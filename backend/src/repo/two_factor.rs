//! Storage of two-factor sign-in: the sealed secret and the step of the
//! last accepted code on the account, and one-time recovery codes.

use sqlx::PgExecutor;
use uuid::Uuid;

/// What an account's two-factor sign-in is at.
#[derive(Debug, sqlx::FromRow)]
pub struct Stored {
    /// The sealed secret: of a setup in progress, or of the enabled factor.
    pub secret: Option<Vec<u8>>,
    pub enabled: bool,
    pub last_step: Option<i64>,
}

pub async fn find(db: impl PgExecutor<'_>, user: Uuid) -> Result<Option<Stored>, sqlx::Error> {
    sqlx::query_as(
        "SELECT totp_secret_enc AS secret, totp_enabled_at IS NOT NULL AS enabled,
                totp_last_step AS last_step
         FROM users WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(user)
    .fetch_optional(db)
    .await
}

/// Keeps a new secret that is not in force until a code proves the app has it.
pub async fn begin(db: impl PgExecutor<'_>, user: Uuid, sealed: &[u8]) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE users SET totp_secret_enc = $2, totp_enabled_at = NULL, totp_last_step = NULL
         WHERE id = $1",
    )
    .bind(user)
    .bind(sealed)
    .execute(db)
    .await?;
    Ok(())
}

pub async fn enable(db: impl PgExecutor<'_>, user: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET totp_enabled_at = now() WHERE id = $1")
        .bind(user)
        .execute(db)
        .await?;
    Ok(())
}

/// Marks the time step of a code as used. False when that step, or a later
/// one, was used before: the same code never works twice, even at once.
pub async fn accept_step(
    db: impl PgExecutor<'_>,
    user: Uuid,
    step: i64,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE users SET totp_last_step = $2
         WHERE id = $1 AND (totp_last_step IS NULL OR totp_last_step < $2)",
    )
    .bind(user)
    .bind(step)
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Turns two-factor sign-in off and removes the recovery codes.
pub async fn clear(db: &mut sqlx::PgConnection, user: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE users SET totp_secret_enc = NULL, totp_enabled_at = NULL, totp_last_step = NULL
         WHERE id = $1",
    )
    .bind(user)
    .execute(&mut *db)
    .await?;
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
        .bind(user)
        .execute(&mut *db)
        .await?;
    Ok(())
}

/// Replaces an account's recovery codes with new ones, given as digests.
pub async fn replace_codes(
    db: &mut sqlx::PgConnection,
    user: Uuid,
    hashes: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM recovery_codes WHERE user_id = $1")
        .bind(user)
        .execute(&mut *db)
        .await?;
    for hash in hashes {
        sqlx::query("INSERT INTO recovery_codes (id, user_id, code_hash) VALUES ($1, $2, $3)")
            .bind(Uuid::now_v7())
            .bind(user)
            .bind(hash)
            .execute(&mut *db)
            .await?;
    }
    Ok(())
}

/// Uses a recovery code up; false when it is not one of the account's unused codes.
pub async fn use_code(
    db: impl PgExecutor<'_>,
    user: Uuid,
    hash: &str,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE recovery_codes SET used_at = now()
         WHERE user_id = $1 AND code_hash = $2 AND used_at IS NULL",
    )
    .bind(user)
    .bind(hash)
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

pub async fn codes_left(db: impl PgExecutor<'_>, user: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM recovery_codes WHERE user_id = $1 AND used_at IS NULL")
        .bind(user)
        .fetch_one(db)
        .await
}
