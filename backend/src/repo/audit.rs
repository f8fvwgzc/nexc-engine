//! The audit log of a workspace: appended to only. The database chains every
//! entry as it is written (migration `20261020000201_audit_chain`) and refuses
//! to change or remove one outside the purge paths (`repo::evidence`).

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::audit::{AuditAction, AuditEntry};
use crate::domain::audit_chain::ChainedEntry;

fn bytes<const N: usize>(row: &PgRow, col: &str) -> Result<[u8; N], sqlx::Error> {
    let raw: Vec<u8> = row.try_get(col)?;
    raw.try_into().map_err(|_| sqlx::Error::ColumnDecode {
        index: col.to_owned(),
        source: format!("{col} is not {N} bytes").into(),
    })
}

impl FromRow<'_, PgRow> for ChainedEntry {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let salt: Option<Vec<u8>> = row.try_get("salt")?;
        Ok(ChainedEntry {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            seq: row.try_get("seq")?,
            action: row.try_get("action")?,
            actor_id: row.try_get("actor_id")?,
            actor_name: row.try_get("actor_name")?,
            subject: row.try_get("subject")?,
            detail: row.try_get("detail")?,
            created_at: row.try_get("created_at")?,
            salt: match salt {
                Some(_) => Some(bytes(row, "salt")?),
                None => None,
            },
            actor_digest: bytes(row, "actor_digest")?,
            subject_digest: bytes(row, "subject_digest")?,
            prev_hash: bytes(row, "prev_hash")?,
            hash: bytes(row, "hash")?,
        })
    }
}

/// The columns of a chained entry (SQL stays static).
macro_rules! chained_columns {
    () => {
        "id, workspace_id, seq, action, actor_id, actor_name, subject, detail, created_at, salt,
         actor_digest, subject_digest, prev_hash, hash"
    };
}

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

/// Appends an entry nobody made: the platform acted on its own (a worker
/// switched a destination off). The actor is `null`, named `system`.
pub async fn record_system(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    action: AuditAction,
    subject: &str,
    detail: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO audit_log (id, workspace_id, actor_id, actor_name, action, subject, detail)
         VALUES ($1, $2, NULL, 'system', $3, $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(workspace_id)
    .bind(action.as_str())
    .bind(subject)
    .bind(detail)
    .execute(db)
    .await?;
    Ok(())
}

/// Entries of a workspace written after `since` and up to `until`, oldest
/// first (the SIEM export's order), starting after `(after_at, after_id)`
/// when given; at most `limit`.
pub async fn export_page(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    since: DateTime<Utc>,
    until: DateTime<Utc>,
    after: Option<(DateTime<Utc>, Uuid)>,
    limit: i64,
) -> Result<Vec<AuditEntry>, sqlx::Error> {
    let (after_at, after_id) = after.unzip();
    sqlx::query_as(
        "SELECT id, action, actor_id, actor_name, subject, detail, created_at
         FROM audit_log
         WHERE workspace_id = $1 AND created_at > $2 AND created_at <= $3
           AND ($4::timestamptz IS NULL OR (created_at, id) > ($4, $5))
         ORDER BY created_at, id LIMIT $6",
    )
    .bind(workspace_id)
    .bind(since)
    .bind(until)
    .bind(after_at)
    .bind(after_id)
    .bind(limit)
    .fetch_all(db)
    .await
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

/// Up to `limit` entries of a workspace's chain in chain order, after
/// position `after` (from the start when `None`).
pub async fn chain_page(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    after: Option<i64>,
    limit: i64,
) -> Result<Vec<ChainedEntry>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        chained_columns!(),
        " FROM audit_log WHERE workspace_id = $1 AND seq > COALESCE($2, -1) ORDER BY seq LIMIT $3"
    ))
    .bind(workspace_id)
    .bind(after)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// The head of a workspace's chain as the database records it: the last
/// entry's position and hash; `None` before the first entry.
pub async fn head(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Option<(i64, [u8; 32])>, sqlx::Error> {
    let row = sqlx::query("SELECT seq, hash FROM audit_chain_heads WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_optional(db)
        .await?;
    row.filter(|row| row.try_get::<i64, _>("seq").is_ok_and(|seq| seq >= 0))
        .map(|row| Ok((row.try_get("seq")?, bytes(&row, "hash")?)))
        .transpose()
}

/// The entries of a workspace that are about a run or its graph: they name
/// the run as their subject, or the run or the graph in their detail. All of
/// them when there are at most `limit`; past that, the run's seal, the
/// graph's deletion and purge first, then the newest. In chain order, with
/// how many there are in all.
pub async fn about_run(
    db: &sqlx::PgPool,
    workspace_id: Uuid,
    run_id: Uuid,
    graph_id: Uuid,
    limit: i64,
) -> Result<(Vec<ChainedEntry>, i64), sqlx::Error> {
    let entries = sqlx::query_as(concat!(
        "SELECT ",
        chained_columns!(),
        " FROM (SELECT * FROM audit_log
                WHERE workspace_id = $1
                  AND (subject = $2::text OR strpos(detail, $2::text) > 0
                       OR strpos(detail, $3::text) > 0)
                ORDER BY action IN ('trace_sealed', 'graph_deleted', 'evidence_purged') DESC,
                         seq DESC
                LIMIT $4) picked
         ORDER BY seq"
    ))
    .bind(workspace_id)
    .bind(run_id)
    .bind(graph_id)
    .bind(limit)
    .fetch_all(db)
    .await?;
    let matching: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_log
         WHERE workspace_id = $1
           AND (subject = $2::text OR strpos(detail, $2::text) > 0 OR strpos(detail, $3::text) > 0)",
    )
    .bind(workspace_id)
    .bind(run_id)
    .bind(graph_id)
    .fetch_one(db)
    .await?;
    Ok((entries, matching))
}
