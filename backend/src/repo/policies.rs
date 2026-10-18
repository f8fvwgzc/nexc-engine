//! Policy versions (`policies`): appended, never changed.

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::policy_document::{Policy, PolicyDocument, PolicyScope};

impl FromRow<'_, PgRow> for Policy {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let document: serde_json::Value = row.try_get("document")?;
        let document = serde_json::from_value(document).map_err(|e| sqlx::Error::ColumnDecode {
            index: "document".into(),
            source: Box::new(e),
        })?;
        Ok(Policy {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            scope: enum_col(row, "scope")?,
            scope_id: row.try_get("scope_id")?,
            version: row.try_get("version")?,
            document,
            author_id: row.try_get("author_id")?,
            author_name: row.try_get("author_name")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// The columns of a version joined with its author (SQL stays static).
macro_rules! policy_columns {
    () => {
        "p.id, p.workspace_id, p.scope, p.scope_id, p.version, p.document, p.author_id,
         COALESCE(u.name, '') AS author_name, p.created_at"
    };
}

/// Every version of a workspace's policies, newest first; `scope` and
/// `scope_id` narrow the list to one level.
pub async fn list(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    scope: Option<PolicyScope>,
    scope_id: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Policy>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        policy_columns!(),
        " FROM policies p LEFT JOIN users u ON u.id = p.author_id
         WHERE p.workspace_id = $1
           AND ($2::text IS NULL OR p.scope = $2)
           AND ($3::uuid IS NULL OR p.scope_id = $3)
         ORDER BY p.created_at DESC, p.version DESC LIMIT $4"
    ))
    .bind(workspace_id)
    .bind(scope.map(|s| s.as_str()))
    .bind(scope_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// The latest version of every level of a workspace: the workspace's own,
/// each team's and each agent's. Resolution picks the ones that apply.
pub async fn latest_all(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<Policy>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT DISTINCT ON (p.scope, p.scope_id) ",
        policy_columns!(),
        " FROM policies p LEFT JOIN users u ON u.id = p.author_id
         WHERE p.workspace_id = $1
         ORDER BY p.scope, p.scope_id, p.version DESC"
    ))
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// The latest version at one level, if any was set.
pub async fn latest(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    scope: PolicyScope,
    scope_id: Option<Uuid>,
) -> Result<Option<Policy>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT ",
        policy_columns!(),
        " FROM policies p LEFT JOIN users u ON u.id = p.author_id
         WHERE p.workspace_id = $1 AND p.scope = $2 AND p.scope_id IS NOT DISTINCT FROM $3
         ORDER BY p.version DESC LIMIT 1"
    ))
    .bind(workspace_id)
    .bind(scope.as_str())
    .bind(scope_id)
    .fetch_optional(db)
    .await
}

/// Appends a version at one level: the next number after the latest.
pub async fn insert(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    scope: PolicyScope,
    scope_id: Option<Uuid>,
    document: &PolicyDocument,
    author_id: Uuid,
) -> Result<Policy, sqlx::Error> {
    let document = serde_json::to_value(document).expect("a policy document serialises");
    sqlx::query_as(
        "WITH inserted AS (
             INSERT INTO policies (id, workspace_id, scope, scope_id, version, document, author_id)
             SELECT $1, $2, $3, $4,
                    COALESCE((SELECT max(version) FROM policies
                              WHERE workspace_id = $2 AND scope = $3 AND scope_id IS NOT DISTINCT FROM $4), 0) + 1,
                    $5, $6
             RETURNING *)
         SELECT p.id, p.workspace_id, p.scope, p.scope_id, p.version, p.document, p.author_id,
                COALESCE(u.name, '') AS author_name, p.created_at
         FROM inserted p LEFT JOIN users u ON u.id = p.author_id",
    )
    .bind(Uuid::now_v7())
    .bind(workspace_id)
    .bind(scope.as_str())
    .bind(scope_id)
    .bind(document)
    .bind(author_id)
    .fetch_one(db)
    .await
}
