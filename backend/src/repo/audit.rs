//! The audit log of a workspace.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::audit::{AuditAction, AuditEntry};

impl FromRow<'_, PgRow> for AuditEntry {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(AuditEntry {
            id: row.try_get("id")?,
            action: enum_col(row, "action")?,
            actor_id: row.try_get("actor_id")?,
            actor_name: row.try_get("actor_name")?,
            subject: row.try_get("subject")?,
            detail: row.try_get("detail")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Who or what an entry is about.
#[derive(Debug, Clone, Copy)]
pub enum Subject<'a> {
    /// Written as given.
    Text(&'a str),
    /// Written as the user's name and e-mail at this moment.
    User(Uuid),
}

/// Appends an entry; the actor's name is copied from their account.
pub async fn record(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    actor_id: Uuid,
    action: AuditAction,
    subject: Subject<'_>,
    detail: &str,
) -> Result<(), sqlx::Error> {
    let (text, user) = match subject {
        Subject::Text(text) => (Some(text), None),
        Subject::User(id) => (None, Some(id)),
    };
    sqlx::query(
        "INSERT INTO audit_log (id, workspace_id, actor_id, actor_name, action, subject, detail)
         VALUES ($1, $2, $3, COALESCE((SELECT name FROM users WHERE id = $3), ''), $4,
                 COALESCE($5, (SELECT name || ' <' || email || '>' FROM users WHERE id = $6), ''),
                 $7)",
    )
    .bind(Uuid::now_v7())
    .bind(workspace_id)
    .bind(actor_id)
    .bind(action.as_str())
    .bind(text)
    .bind(user)
    .bind(detail)
    .execute(db)
    .await?;
    Ok(())
}

/// Entries of a workspace, newest first, older than `before` if given.
pub async fn list(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    before: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Vec<AuditEntry>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, action, actor_id, actor_name, subject, detail, created_at
         FROM audit_log
         WHERE workspace_id = $1 AND ($2::timestamptz IS NULL OR created_at < $2)
         ORDER BY created_at DESC, id DESC LIMIT $3",
    )
    .bind(workspace_id)
    .bind(before)
    .bind(limit)
    .fetch_all(db)
    .await
}
