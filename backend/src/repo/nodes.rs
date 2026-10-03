//! Graph nodes. Callers verify graph ownership first; every query is also
//! scoped by `graph_id` so a node id from another graph never matches.

use sqlx::postgres::PgRow;
use sqlx::types::Json;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::graph::{GraphNode, NodeDraft, NodeStatus};

impl FromRow<'_, PgRow> for GraphNode {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let Json(tags): Json<Vec<String>> = row.try_get("tags")?;
        Ok(GraphNode {
            id: row.try_get("id")?,
            graph_id: row.try_get("graph_id")?,
            title: row.try_get("title")?,
            content: row.try_get("content")?,
            kind: enum_col(row, "kind")?,
            tags,
            x: row.try_get("x")?,
            y: row.try_get("y")?,
            status: enum_col(row, "status")?,
            agent_role: row.try_get("agent_role")?,
            executor: enum_col(row, "executor")?,
            output: row.try_get("output")?,
            origin: enum_col(row, "origin")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

/// Inserts a node with a caller-chosen id (UUID v7).
pub async fn create(
    db: impl PgExecutor<'_>,
    id: Uuid,
    graph_id: Uuid,
    draft: &NodeDraft,
) -> Result<GraphNode, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO nodes (id, graph_id, title, content, kind, tags, x, y, agent_role, executor, origin)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
         RETURNING *",
    )
    .bind(id)
    .bind(graph_id)
    .bind(&draft.title)
    .bind(&draft.content)
    .bind(draft.kind.as_str())
    .bind(Json(&draft.tags))
    .bind(draft.x)
    .bind(draft.y)
    .bind(&draft.agent_role)
    .bind(draft.executor.as_str())
    .bind(draft.origin.as_str())
    .fetch_one(db)
    .await
}

/// All nodes of a graph in creation order.
pub async fn list(db: impl PgExecutor<'_>, graph_id: Uuid) -> Result<Vec<GraphNode>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM nodes WHERE graph_id = $1 ORDER BY created_at, id")
        .bind(graph_id)
        .fetch_all(db)
        .await
}

/// One node of a graph.
pub async fn find(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
    id: Uuid,
) -> Result<Option<GraphNode>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM nodes WHERE graph_id = $1 AND id = $2")
        .bind(graph_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Number of nodes in a graph.
pub async fn count(db: impl PgExecutor<'_>, graph_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM nodes WHERE graph_id = $1")
        .bind(graph_id)
        .fetch_one(db)
        .await
}

/// Writes every mutable field of `node`.
pub async fn save(db: impl PgExecutor<'_>, node: &GraphNode) -> Result<GraphNode, sqlx::Error> {
    sqlx::query_as(
        "UPDATE nodes SET title = $3, content = $4, kind = $5, tags = $6, x = $7, y = $8, status = $9,
                agent_role = $10, executor = $11, output = $12, updated_at = now()
         WHERE graph_id = $1 AND id = $2 RETURNING *",
    )
    .bind(node.graph_id)
    .bind(node.id)
    .bind(&node.title)
    .bind(&node.content)
    .bind(node.kind.as_str())
    .bind(Json(&node.tags))
    .bind(node.x)
    .bind(node.y)
    .bind(node.status.as_str())
    .bind(&node.agent_role)
    .bind(node.executor.as_str())
    .bind(&node.output)
    .fetch_one(db)
    .await
}

/// Moves a node on the canvas.
pub async fn set_position(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
    id: Uuid,
    x: f64,
    y: f64,
) -> Result<Option<GraphNode>, sqlx::Error> {
    sqlx::query_as("UPDATE nodes SET x = $3, y = $4, updated_at = now() WHERE graph_id = $1 AND id = $2 RETURNING *")
        .bind(graph_id)
        .bind(id)
        .bind(x)
        .bind(y)
        .fetch_optional(db)
        .await
}

/// Records the execution status (and optionally the output) of a node.
pub async fn set_status(
    db: impl PgExecutor<'_>,
    id: Uuid,
    status: NodeStatus,
    output: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE nodes SET status = $2, output = COALESCE($3, output), updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(status.as_str())
    .bind(output)
    .execute(db)
    .await?;
    Ok(())
}

/// Deletes a node (its edges cascade). Returns false if absent.
pub async fn delete(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM nodes WHERE graph_id = $1 AND id = $2")
        .bind(graph_id)
        .bind(id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() == 1)
}
