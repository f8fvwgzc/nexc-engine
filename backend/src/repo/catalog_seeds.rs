//! Which catalog seeds each workspace has received (`planner::seed_catalog`).

use sqlx::PgExecutor;
use uuid::Uuid;

/// Records that `workspace` receives `seed` now; false when it already did,
/// or when another backend is giving it to the workspace at this moment (a
/// transaction-scoped advisory lock, so replicas skip each other instead of
/// waiting).
pub async fn claim(
    db: &mut sqlx::PgConnection,
    workspace: Uuid,
    seed: &str,
) -> Result<bool, sqlx::Error> {
    let locked: bool =
        sqlx::query_scalar("SELECT pg_try_advisory_xact_lock(hashtextextended($1::text || $2, 0))")
            .bind(workspace)
            .bind(seed)
            .fetch_one(&mut *db)
            .await?;
    if !locked {
        return Ok(false);
    }
    let claimed: Option<i32> = sqlx::query_scalar(
        "INSERT INTO workspace_catalog_seeds (workspace_id, seed) VALUES ($1, $2)
         ON CONFLICT DO NOTHING RETURNING 1",
    )
    .bind(workspace)
    .bind(seed)
    .fetch_optional(&mut *db)
    .await?;
    Ok(claimed.is_some())
}

/// Workspaces that have not received `seed`, oldest first, each with the
/// member it is given as: an owner or admin, its creator first, then owners.
pub async fn workspaces_without(
    db: impl PgExecutor<'_>,
    seed: &str,
) -> Result<Vec<(Uuid, Option<Uuid>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT w.id,
                (SELECT m.user_id FROM workspace_members m
                 WHERE m.workspace_id = w.id AND m.role IN ('owner', 'admin')
                 ORDER BY (m.user_id = w.created_by) DESC, (m.role = 'owner') DESC,
                          m.created_at, m.user_id
                 LIMIT 1)
         FROM workspaces w
         WHERE NOT EXISTS (SELECT 1 FROM workspace_catalog_seeds s
                           WHERE s.workspace_id = w.id AND s.seed = $1)
         ORDER BY w.created_at",
    )
    .bind(seed)
    .fetch_all(db)
    .await
}
