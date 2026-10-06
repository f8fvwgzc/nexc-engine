//! Users and login bookkeeping.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::user::{Role, User};

impl FromRow<'_, PgRow> for User {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(User {
            id: row.try_get("id")?,
            email: row.try_get("email")?,
            name: row.try_get("name")?,
            role: enum_col(row, "role")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// A user with its credential state, for login.
#[derive(Debug)]
pub struct Credentials {
    pub user: User,
    pub password_hash: String,
    pub failed_logins: i32,
    pub locked_until: Option<DateTime<Utc>>,
    /// Suspended by a platform administrator.
    pub suspended: bool,
}

/// Inserts a user. Fails with a unique violation when the e-mail exists.
pub async fn create(
    db: impl PgExecutor<'_>,
    email: &str,
    name: &str,
    role: Role,
    password_hash: &str,
) -> Result<User, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO users (id, email, name, role, password_hash) VALUES ($1, $2, $3, $4, $5)
         RETURNING id, email, name, role, created_at",
    )
    .bind(Uuid::now_v7())
    .bind(email)
    .bind(name)
    .bind(role.as_str())
    .bind(password_hash)
    .fetch_one(db)
    .await
}

/// Finds a user by id.
pub async fn find(db: impl PgExecutor<'_>, id: Uuid) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as("SELECT id, email, name, role, created_at FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Finds a user who may hold a session: one that exists and is not suspended.
pub async fn find_active(db: impl PgExecutor<'_>, id: Uuid) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, email, name, role, created_at FROM users
         WHERE id = $1 AND suspended_at IS NULL",
    )
    .bind(id)
    .fetch_optional(db)
    .await
}

/// The session epoch new access tokens of a user carry.
pub async fn session_epoch(db: impl PgExecutor<'_>, id: Uuid) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar("SELECT session_epoch FROM users WHERE id = $1")
        .bind(id)
        .fetch_one(db)
        .await
}

/// Finds credentials by (normalised) e-mail.
pub async fn credentials(
    db: impl PgExecutor<'_>,
    email: &str,
) -> Result<Option<Credentials>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, email, name, role, created_at, password_hash, failed_logins, locked_until,
                suspended_at IS NOT NULL AS suspended
         FROM users WHERE lower(email) = lower($1)",
    )
    .bind(email)
    .fetch_optional(db)
    .await?;
    row.map(|r| {
        Ok(Credentials {
            user: User::from_row(&r)?,
            password_hash: r.try_get("password_hash")?,
            failed_logins: r.try_get("failed_logins")?,
            locked_until: r.try_get("locked_until")?,
            suspended: r.try_get("suspended")?,
        })
    })
    .transpose()
}

/// Records a failed login; returns the new consecutive failure count.
pub async fn record_failed_login(db: impl PgExecutor<'_>, id: Uuid) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE users SET failed_logins = failed_logins + 1, updated_at = now() WHERE id = $1
         RETURNING failed_logins",
    )
    .bind(id)
    .fetch_one(db)
    .await
}

/// Locks the account until `until`.
pub async fn lock_until(
    db: impl PgExecutor<'_>,
    id: Uuid,
    until: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET locked_until = $2 WHERE id = $1")
        .bind(id)
        .bind(until)
        .execute(db)
        .await?;
    Ok(())
}

/// Clears failure counters after a successful login.
pub async fn reset_login_failures(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE users SET failed_logins = 0, locked_until = NULL WHERE id = $1 AND failed_logins > 0")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Whether a user with this e-mail exists.
pub async fn email_exists(db: impl PgExecutor<'_>, email: &str) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE lower(email) = lower($1))")
        .bind(email)
        .fetch_one(db)
        .await
}

/// Ids of every user (for start-up seeding).
pub async fn all_ids(db: impl PgExecutor<'_>) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT id FROM users ORDER BY created_at")
        .fetch_all(db)
        .await
}
