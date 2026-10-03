//! Graph headers and summaries.

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use crate::domain::graph::{GraphMeta, GraphSummary};

impl FromRow<'_, PgRow> for GraphMeta {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(GraphMeta {
            id: row.try_get("id")?,
            owner_id: row.try_get("owner_id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            goal: row.try_get("goal")?,
            version: row.try_get("version")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl FromRow<'_, PgRow> for GraphSummary {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(GraphSummary {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            node_count: row.try_get("node_count")?,
            edge_count: row.try_get("edge_count")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

/// Creates a graph.
pub async fn create(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
    name: &str,
    description: &str,
    goal: &str,
) -> Result<GraphMeta, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO graphs (id, owner_id, name, description, goal) VALUES ($1, $2, $3, $4, $5)
         RETURNING id, owner_id, name, description, goal, version, created_at, updated_at",
    )
    .bind(Uuid::now_v7())
    .bind(owner_id)
    .bind(name)
    .bind(description)
    .bind(goal)
    .fetch_one(db)
    .await
}

/// Loads a graph header owned by `owner_id`.
pub async fn find(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
    id: Uuid,
) -> Result<Option<GraphMeta>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, owner_id, name, description, goal, version, created_at, updated_at
         FROM graphs WHERE id = $1 AND owner_id = $2",
    )
    .bind(id)
    .bind(owner_id)
    .fetch_optional(db)
    .await
}

/// Loads and row-locks a graph header (serialises structural edits such as
/// edge insertion with cycle checks). Must run inside a transaction.
pub async fn lock(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
    id: Uuid,
) -> Result<Option<GraphMeta>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, owner_id, name, description, goal, version, created_at, updated_at
         FROM graphs WHERE id = $1 AND owner_id = $2 FOR UPDATE",
    )
    .bind(id)
    .bind(owner_id)
    .fetch_optional(db)
    .await
}

/// Lists the owner's graphs, most recently updated first.
pub async fn list(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
) -> Result<Vec<GraphSummary>, sqlx::Error> {
    sqlx::query_as(
        "SELECT g.id, g.name, g.description, g.updated_at,
                (SELECT count(*) FROM nodes n WHERE n.graph_id = g.id) AS node_count,
                (SELECT count(*) FROM edges e WHERE e.graph_id = g.id) AS edge_count
         FROM graphs g WHERE g.owner_id = $1 ORDER BY g.updated_at DESC",
    )
    .bind(owner_id)
    .fetch_all(db)
    .await
}

/// Summary of one graph (for realtime `graph.updated`).
pub async fn summary(
    db: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<GraphSummary>, sqlx::Error> {
    sqlx::query_as(
        "SELECT g.id, g.name, g.description, g.updated_at,
                (SELECT count(*) FROM nodes n WHERE n.graph_id = g.id) AS node_count,
                (SELECT count(*) FROM edges e WHERE e.graph_id = g.id) AS edge_count
         FROM graphs g WHERE g.id = $1",
    )
    .bind(id)
    .fetch_optional(db)
    .await
}

/// Updates name / description / goal (each optional) and bumps the version.
pub async fn update(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
    id: Uuid,
    name: Option<&str>,
    description: Option<&str>,
    goal: Option<&str>,
) -> Result<Option<GraphMeta>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE graphs SET name = COALESCE($3, name), description = COALESCE($4, description),
                goal = COALESCE($5, goal), version = version + 1, updated_at = now()
         WHERE id = $1 AND owner_id = $2
         RETURNING id, owner_id, name, description, goal, version, created_at, updated_at",
    )
    .bind(id)
    .bind(owner_id)
    .bind(name)
    .bind(description)
    .bind(goal)
    .fetch_optional(db)
    .await
}

/// Increments the version after a node / edge mutation.
pub async fn touch(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE graphs SET version = version + 1, updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Deletes a graph (cascades to nodes, edges, plans, runs). Returns false if absent.
pub async fn delete(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM graphs WHERE id = $1 AND owner_id = $2")
        .bind(id)
        .bind(owner_id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() == 1)
}
