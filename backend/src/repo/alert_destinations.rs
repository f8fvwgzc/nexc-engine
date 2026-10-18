//! Alert destinations of a workspace (`alert_destinations`): registered by
//! admins, read by the alert routing worker. The URL and the signing secret
//! are stored encrypted and only handed out as the sealed bytes.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::alert::AlertKind;
use crate::domain::alert_routing::{AlertDestination, DestinationKind};
use crate::domain::policy_document::Severity;

/// A destination row with its sealed URL and secret, for the worker only.
#[derive(Debug, Clone)]
pub struct AlertDestinationRow {
    pub destination: AlertDestination,
    /// AES-256-GCM under the master key, bound to the destination's id.
    pub url: Vec<u8>,
    /// AES-256-GCM under the master key, bound to the destination's id.
    pub secret: Vec<u8>,
}

impl FromRow<'_, PgRow> for AlertDestinationRow {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let kinds: serde_json::Value = row.try_get("kinds")?;
        Ok(AlertDestinationRow {
            destination: AlertDestination {
                id: row.try_get("id")?,
                workspace_id: row.try_get("workspace_id")?,
                name: row.try_get("name")?,
                kind: enum_col(row, "kind")?,
                host: row.try_get("host")?,
                kinds: serde_json::from_value::<Vec<AlertKind>>(kinds).unwrap_or_default(),
                min_severity: enum_col(row, "min_severity")?,
                include_approvals: row.try_get("include_approvals")?,
                enabled: row.try_get("enabled")?,
                disabled_reason: row.try_get("disabled_reason")?,
                failures: row.try_get("failures")?,
                last_delivered_at: row.try_get("last_delivered_at")?,
                last_error: row.try_get("last_error")?,
                created_by: row.try_get("created_by")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            },
            url: row.try_get("url")?,
            secret: row.try_get("secret")?,
        })
    }
}

/// What a destination is registered with.
#[derive(Debug, Clone)]
pub struct NewDestination<'a> {
    pub workspace_id: Uuid,
    pub name: &'a str,
    pub kind: DestinationKind,
    pub url: &'a [u8],
    pub host: &'a str,
    pub secret: &'a [u8],
    pub kinds: &'a [AlertKind],
    pub min_severity: Severity,
    pub include_approvals: bool,
    pub enabled: bool,
    pub created_by: Uuid,
}

/// What a patch changes; `None` keeps a field.
#[derive(Debug, Clone, Default)]
pub struct DestinationChanges<'a> {
    pub name: Option<&'a str>,
    /// The sealed URL and its host.
    pub url: Option<(&'a [u8], &'a str)>,
    pub kinds: Option<&'a [AlertKind]>,
    pub min_severity: Option<Severity>,
    pub include_approvals: Option<bool>,
    pub enabled: Option<bool>,
}

/// Registers a destination; the id is chosen here so the secrets can be bound to it.
pub async fn insert(
    db: impl PgExecutor<'_>,
    id: Uuid,
    new: &NewDestination<'_>,
) -> Result<AlertDestinationRow, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO alert_destinations (id, workspace_id, name, kind, url, host, secret, kinds,
                                         min_severity, include_approvals, enabled, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) RETURNING *",
    )
    .bind(id)
    .bind(new.workspace_id)
    .bind(new.name)
    .bind(new.kind.as_str())
    .bind(new.url)
    .bind(new.host)
    .bind(new.secret)
    .bind(serde_json::to_value(new.kinds).expect("kinds serialise"))
    .bind(new.min_severity.as_str())
    .bind(new.include_approvals)
    .bind(new.enabled)
    .bind(new.created_by)
    .fetch_one(db)
    .await
}

/// The destinations of a workspace, by name.
pub async fn list(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<AlertDestinationRow>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM alert_destinations WHERE workspace_id = $1 ORDER BY name, id")
        .bind(workspace_id)
        .fetch_all(db)
        .await
}

/// One destination of a workspace.
pub async fn find(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<AlertDestinationRow>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM alert_destinations WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// One destination by id, whatever its workspace (the worker's view).
pub async fn find_any(
    db: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<AlertDestinationRow>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM alert_destinations WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Applies a patch; `None` when the destination is not there. Enabling a
/// destination clears its failure count and the reason it was switched off.
pub async fn update(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
    changes: &DestinationChanges<'_>,
) -> Result<Option<AlertDestinationRow>, sqlx::Error> {
    let (url, host) = changes.url.unzip();
    sqlx::query_as(
        "UPDATE alert_destinations SET
            name = COALESCE($3, name),
            url = COALESCE($4, url),
            host = COALESCE($5, host),
            kinds = COALESCE($6, kinds),
            min_severity = COALESCE($7, min_severity),
            include_approvals = COALESCE($8, include_approvals),
            enabled = COALESCE($9, enabled),
            failures = CASE WHEN $9 THEN 0 ELSE failures END,
            disabled_reason = CASE WHEN $9 THEN '' ELSE disabled_reason END,
            updated_at = now()
         WHERE workspace_id = $1 AND id = $2 RETURNING *",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(changes.name)
    .bind(url)
    .bind(host)
    .bind(
        changes
            .kinds
            .map(|k| serde_json::to_value(k).expect("kinds serialise")),
    )
    .bind(changes.min_severity.map(Severity::as_str))
    .bind(changes.include_approvals)
    .bind(changes.enabled)
    .fetch_optional(db)
    .await
}

/// Removes a destination (its deliveries go with it); false when it was not there.
pub async fn delete(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM alert_destinations WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// Notes a delivery that went through: the failure count starts over.
pub async fn record_success(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE alert_destinations SET failures = 0, last_delivered_at = now(), last_error = '',
                updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

/// Notes a delivery that did not go through; returns the failures in a row now.
pub async fn record_failure(
    db: impl PgExecutor<'_>,
    id: Uuid,
    error: &str,
) -> Result<i32, sqlx::Error> {
    sqlx::query_scalar(
        "UPDATE alert_destinations SET failures = failures + 1, last_error = $2, updated_at = now()
         WHERE id = $1 RETURNING failures",
    )
    .bind(id)
    .bind(error)
    .fetch_one(db)
    .await
}

/// Switches a destination off with the reason; false when it was off already.
pub async fn disable(db: impl PgExecutor<'_>, id: Uuid, reason: &str) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE alert_destinations SET enabled = false, disabled_reason = $2, updated_at = now()
         WHERE id = $1 AND enabled",
    )
    .bind(id)
    .bind(reason)
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// The name and slug of a workspace, for the deliveries' envelope.
pub async fn workspace_label(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Option<(String, String)>, sqlx::Error> {
    sqlx::query_as("SELECT name, slug FROM workspaces WHERE id = $1")
        .bind(workspace_id)
        .fetch_optional(db)
        .await
}

/// When the destination's latest delivery was tried, for the console.
pub async fn last_attempt(
    db: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<DateTime<Utc>>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT max(last_attempt_at) FROM alert_deliveries WHERE destination_id = $1",
    )
    .bind(id)
    .fetch_one(db)
    .await
}
