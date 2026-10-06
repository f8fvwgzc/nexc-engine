//! The security activity of accounts: appended as things happen, read by
//! the account's holder, dropped after `ACTIVITY_KEPT_DAYS`.

use std::net::IpAddr;

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::account::{ACTIVITY_KEPT_DAYS, AccountEvent, AccountEventKind};

impl FromRow<'_, PgRow> for AccountEvent {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(AccountEvent {
            id: row.try_get("id")?,
            kind: enum_col(row, "kind")?,
            ip: row.try_get("ip")?,
            detail: row.try_get("detail")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Appends an entry. An address that says nothing (unspecified) is left out.
pub async fn record(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    kind: AccountEventKind,
    ip: Option<IpAddr>,
    detail: &str,
) -> Result<(), sqlx::Error> {
    let ip = ip
        .filter(|ip| !ip.is_unspecified())
        .map(|ip| ip.to_string());
    sqlx::query(
        "INSERT INTO account_events (id, user_id, kind, ip, detail) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(user_id)
    .bind(kind.as_str())
    .bind(ip)
    .bind(detail)
    .execute(db)
    .await?;
    Ok(())
}

/// The latest entries of an account, newest first.
pub async fn list(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    limit: i64,
) -> Result<Vec<AccountEvent>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, kind, ip, detail, created_at FROM account_events
         WHERE user_id = $1 ORDER BY created_at DESC, id DESC LIMIT $2",
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Deletes entries older than they are kept for.
pub async fn purge(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "DELETE FROM account_events WHERE created_at < now() - make_interval(days => $1)",
    )
    .bind(ACTIVITY_KEPT_DAYS)
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}
