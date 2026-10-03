//! Graph edges.

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::graph::{EdgeKind, EdgeOrigin, GraphEdge};

impl FromRow<'_, PgRow> for GraphEdge {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(GraphEdge {
            id: row.try_get("id")?,
            graph_id: row.try_get("graph_id")?,
            source: row.try_get("source")?,
            target: row.try_get("target")?,
            kind: enum_col(row, "kind")?,
            origin: enum_col(row, "origin")?,
            weight: row.try_get("weight")?,
        })
    }
}

/// Inserts an edge; returns `None` when the same (source, target, kind) exists.
pub async fn create(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
    source: Uuid,
    target: Uuid,
    kind: EdgeKind,
    origin: EdgeOrigin,
) -> Result<Option<GraphEdge>, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO edges (id, graph_id, source, target, kind, origin) VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (source, target, kind) DO NOTHING
         RETURNING id, graph_id, source, target, kind, origin, weight",
    )
    .bind(Uuid::now_v7())
    .bind(graph_id)
    .bind(source)
    .bind(target)
    .bind(kind.as_str())
    .bind(origin.as_str())
    .fetch_optional(db)
    .await
}

/// All edges of a graph.
pub async fn list(db: impl PgExecutor<'_>, graph_id: Uuid) -> Result<Vec<GraphEdge>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, graph_id, source, target, kind, origin, weight FROM edges WHERE graph_id = $1 ORDER BY created_at, id",
    )
    .bind(graph_id)
    .fetch_all(db)
    .await
}

/// Deletes one edge of a graph. Returns false if absent.
pub async fn delete(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM edges WHERE graph_id = $1 AND id = $2")
        .bind(graph_id)
        .bind(id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// Deletes every edge of `origin` in a graph; returns the deleted ids.
pub async fn delete_by_origin(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
    origin: EdgeOrigin,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar("DELETE FROM edges WHERE graph_id = $1 AND origin = $2 RETURNING id")
        .bind(graph_id)
        .bind(origin.as_str())
        .fetch_all(db)
        .await
}
