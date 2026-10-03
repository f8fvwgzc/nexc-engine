//! Plans proposed by the planner.

use sqlx::postgres::PgRow;
use sqlx::types::Json;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::plan::{Plan, PlanProposal, PlanStatus, ProposedEdge, ProposedNode};

impl FromRow<'_, PgRow> for Plan {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let Json(nodes): Json<Vec<ProposedNode>> = row.try_get("nodes")?;
        let Json(edges): Json<Vec<ProposedEdge>> = row.try_get("edges")?;
        Ok(Plan {
            id: row.try_get("id")?,
            graph_id: row.try_get("graph_id")?,
            status: enum_col(row, "status")?,
            summary: row.try_get("summary")?,
            nodes,
            edges,
            error: row.try_get("error")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// Creates a plan in `streaming` state.
pub async fn create(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
    owner_id: Uuid,
    instructions: &str,
) -> Result<Plan, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO plans (id, graph_id, owner_id, status, instructions) VALUES ($1, $2, $3, 'streaming', $4)
         RETURNING id, graph_id, status, summary, nodes, edges, error, created_at",
    )
    .bind(Uuid::now_v7())
    .bind(graph_id)
    .bind(owner_id)
    .bind(instructions)
    .fetch_one(db)
    .await
}

/// Loads a plan of a graph.
pub async fn find(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
    id: Uuid,
) -> Result<Option<Plan>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, graph_id, status, summary, nodes, edges, error, created_at FROM plans WHERE graph_id = $1 AND id = $2",
    )
    .bind(graph_id)
    .bind(id)
    .fetch_optional(db)
    .await
}

/// Stores a finished proposal and marks the plan `ready`.
pub async fn complete(
    db: impl PgExecutor<'_>,
    id: Uuid,
    proposal: &PlanProposal,
) -> Result<Plan, sqlx::Error> {
    sqlx::query_as(
        "UPDATE plans SET status = 'ready', summary = $2, nodes = $3, edges = $4, updated_at = now() WHERE id = $1
         RETURNING id, graph_id, status, summary, nodes, edges, error, created_at",
    )
    .bind(id)
    .bind(&proposal.summary)
    .bind(Json(&proposal.nodes))
    .bind(Json(&proposal.edges))
    .fetch_one(db)
    .await
}

/// Marks the plan failed with `error`.
pub async fn fail(db: impl PgExecutor<'_>, id: Uuid, error: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE plans SET status = 'failed', error = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(error)
        .execute(db)
        .await?;
    Ok(())
}

/// Moves a plan from `from` to `to`; returns false if it was not in `from`
/// (makes `apply` idempotent and race free).
pub async fn transition(
    db: impl PgExecutor<'_>,
    id: Uuid,
    from: PlanStatus,
    to: PlanStatus,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE plans SET status = $3, updated_at = now() WHERE id = $1 AND status = $2",
    )
    .bind(id)
    .bind(from.as_str())
    .bind(to.as_str())
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Fails plans left streaming by a crashed process (older than 15 minutes).
pub async fn fail_stale(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE plans SET status = 'failed', error = 'interrupted', updated_at = now()
         WHERE status = 'streaming' AND updated_at < now() - interval '15 minutes'",
    )
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}
