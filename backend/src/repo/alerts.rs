//! Alerts (`alerts`): raised by the engine, acknowledged by admins.

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::alert::{Alert, AlertKind};
use crate::domain::policy_document::Severity;

impl FromRow<'_, PgRow> for Alert {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Alert {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            agent_id: row.try_get("agent_id")?,
            principal: row.try_get("principal")?,
            run_id: row.try_get("run_id")?,
            node_id: row.try_get("node_id")?,
            graph_id: row.try_get("graph_id")?,
            span_id: row.try_get("span_id")?,
            kind: enum_col(row, "kind")?,
            severity: enum_col(row, "severity")?,
            rule: row.try_get("rule")?,
            title: row.try_get("title")?,
            detail: row.try_get("detail")?,
            created_at: row.try_get("created_at")?,
            acknowledged_at: row.try_get("acknowledged_at")?,
            acknowledged_by: row.try_get("acknowledged_by")?,
            deliveries: row.try_get("deliveries").unwrap_or(0),
        })
    }
}

/// What raises an alert.
#[derive(Debug, Clone)]
pub struct NewAlert<'a> {
    pub workspace_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub principal: &'a str,
    pub run_id: Option<Uuid>,
    pub node_id: Option<Uuid>,
    pub graph_id: Option<Uuid>,
    pub span_id: Option<Uuid>,
    pub kind: AlertKind,
    pub severity: Severity,
    pub rule: &'a str,
    pub title: &'a str,
    pub detail: &'a str,
}

/// Records an alert.
pub async fn insert(db: impl PgExecutor<'_>, a: &NewAlert<'_>) -> Result<Alert, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO alerts (id, workspace_id, agent_id, principal, run_id, node_id, graph_id, span_id,
                             kind, severity, rule, title, detail)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(a.workspace_id)
    .bind(a.agent_id)
    .bind(a.principal)
    .bind(a.run_id)
    .bind(a.node_id)
    .bind(a.graph_id)
    .bind(a.span_id)
    .bind(a.kind.as_str())
    .bind(a.severity.as_str())
    .bind(a.rule)
    .bind(a.title)
    .bind(a.detail)
    .fetch_one(db)
    .await
}

/// Alerts of a workspace, newest first, older than `before` if given.
pub async fn list(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    before: Option<chrono::DateTime<chrono::Utc>>,
    limit: i64,
) -> Result<Vec<Alert>, sqlx::Error> {
    sqlx::query_as(
        "SELECT a.*, (SELECT count(*) FROM alert_deliveries d
                      WHERE d.alert_id = a.id AND d.status = 'delivered') AS deliveries
         FROM alerts a
         WHERE a.workspace_id = $1 AND ($2::timestamptz IS NULL OR a.created_at < $2)
         ORDER BY a.created_at DESC, a.id DESC LIMIT $3",
    )
    .bind(workspace_id)
    .bind(before)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Alerts of a workspace raised after `since` and up to `until`, oldest
/// first (the SIEM export's order), starting after `(after_at, after_id)`
/// when given; at most `limit`.
pub async fn export_page(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    since: chrono::DateTime<chrono::Utc>,
    until: chrono::DateTime<chrono::Utc>,
    after: Option<(chrono::DateTime<chrono::Utc>, Uuid)>,
    limit: i64,
) -> Result<Vec<Alert>, sqlx::Error> {
    let (after_at, after_id) = after.unzip();
    sqlx::query_as(
        "SELECT a.*, (SELECT count(*) FROM alert_deliveries d
                      WHERE d.alert_id = a.id AND d.status = 'delivered') AS deliveries
         FROM alerts a
         WHERE a.workspace_id = $1 AND a.created_at > $2 AND a.created_at <= $3
           AND ($4::timestamptz IS NULL OR (a.created_at, a.id) > ($4, $5))
         ORDER BY a.created_at, a.id LIMIT $6",
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

/// One alert by id.
pub async fn find(db: impl PgExecutor<'_>, id: Uuid) -> Result<Option<Alert>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM alerts WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Marks an alert seen; `None` when it was acknowledged already or is unknown.
pub async fn acknowledge(
    db: impl PgExecutor<'_>,
    id: Uuid,
    user_id: Uuid,
) -> Result<Option<Alert>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE alerts SET acknowledged_at = now(), acknowledged_by = $2
         WHERE id = $1 AND acknowledged_at IS NULL RETURNING *",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(db)
    .await
}

/// Puts an alert into the inbox of each of `user_ids`.
pub async fn notify(
    db: impl PgExecutor<'_>,
    alert_id: Uuid,
    workspace_id: Uuid,
    user_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    if user_ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO notifications (id, user_id, workspace_id, alert_id, kind)
         SELECT gen_random_uuid(), u, $2, $1, 'alert' FROM unnest($3::uuid[]) AS u",
    )
    .bind(alert_id)
    .bind(workspace_id)
    .bind(user_ids)
    .execute(db)
    .await?;
    Ok(())
}
