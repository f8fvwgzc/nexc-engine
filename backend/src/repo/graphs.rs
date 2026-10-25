//! Graph headers and summaries. A graph's vocabulary is not here: it lives
//! in onto-intelli (`ontology::vocabulary`); the `ontology` column is only
//! read once, to copy it there (`repo::onto_migrations`).

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use crate::domain::graph::{GraphMeta, GraphSummary};

impl FromRow<'_, PgRow> for GraphMeta {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(GraphMeta {
            id: row.try_get("id")?,
            owner_id: row.try_get("owner_id")?,
            workspace_id: row.try_get("workspace_id")?,
            team_id: row.try_get("team_id")?,
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
            workspace_id: row.try_get("workspace_id")?,
            team_id: row.try_get("team_id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            node_count: row.try_get("node_count")?,
            edge_count: row.try_get("edge_count")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

/// Creates a graph in a workspace (and optionally one of its teams); its
/// vocabulary starts as the starter set in onto-intelli on first read.
pub async fn create(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
    workspace_id: Uuid,
    team_id: Option<Uuid>,
    name: &str,
    description: &str,
    goal: &str,
) -> Result<GraphMeta, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO graphs (id, owner_id, name, description, goal, workspace_id, team_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id, owner_id, workspace_id, team_id, name, description, goal, version, created_at, updated_at",
    )
    .bind(Uuid::now_v7())
    .bind(owner_id)
    .bind(name)
    .bind(description)
    .bind(goal)
    .bind(workspace_id)
    .bind(team_id)
    .fetch_one(db)
    .await
}

/// Loads a graph header that `owner_id` may work on (see `graph_access!`).
pub async fn find(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
    id: Uuid,
) -> Result<Option<GraphMeta>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT id, owner_id, workspace_id, team_id, name, description, goal, version, created_at, updated_at
         FROM graphs WHERE id = $1 AND ",
        graph_access!("graphs", "$2")
    ))
    .bind(id)
    .bind(owner_id)
    .fetch_optional(db)
    .await
}

/// A graph header, whoever may work on it (engine use; callers hold the
/// graph id from a row they were already allowed to read).
pub async fn header(db: impl PgExecutor<'_>, id: Uuid) -> Result<Option<GraphMeta>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, owner_id, workspace_id, team_id, name, description, goal, version, created_at, updated_at
         FROM graphs WHERE id = $1",
    )
    .bind(id)
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
    sqlx::query_as(concat!(
        "SELECT id, owner_id, workspace_id, team_id, name, description, goal, version, created_at, updated_at
         FROM graphs WHERE id = $1 AND ",
        graph_access!("graphs", "$2"),
        " FOR UPDATE"
    ))
    .bind(id)
    .bind(owner_id)
    .fetch_optional(db)
    .await
}

/// The graphs `user_id` may work on, optionally only those of one
/// workspace, most recently updated first; the first `limit` of them when it
/// is given. The node and edge counts are read for those rows only.
pub async fn list(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Option<Uuid>,
    limit: Option<i64>,
) -> Result<Vec<GraphSummary>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT g.id, g.workspace_id, g.team_id, g.name, g.description, g.updated_at,
                (SELECT count(*) FROM nodes n WHERE n.graph_id = g.id) AS node_count,
                (SELECT count(*) FROM edges e WHERE e.graph_id = g.id) AS edge_count
         FROM graphs g WHERE ($2::uuid IS NULL OR g.workspace_id = $2) AND ",
        graph_access!("g", "$1"),
        " ORDER BY g.updated_at DESC, g.id LIMIT $3"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Graphs of a workspace that `user_id` may work on and whose name contains
/// `q`, most recently updated first, as `(id, name, goal)`.
pub async fn search(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    q: &str,
    limit: i64,
) -> Result<Vec<(Uuid, String, String)>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT g.id, g.name, left(COALESCE(g.goal, ''), 120)
         FROM graphs g
         WHERE g.workspace_id = $2 AND g.name ILIKE '%' || $3 || '%' AND ",
        graph_access!("g", "$1"),
        " ORDER BY g.updated_at DESC, g.id LIMIT $4"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .bind(q)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Ids of the graphs of a workspace that `user_id` may work on.
pub async fn accessible_ids(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(concat!(
        "SELECT g.id FROM graphs g WHERE g.workspace_id = $2 AND ",
        graph_access!("g", "$1")
    ))
    .bind(user_id)
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// Assigns graphs created before workspaces existed to their creator's first workspace.
pub async fn adopt_orphans(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE graphs g SET workspace_id = (
            SELECT m.workspace_id FROM workspace_members m
            WHERE m.user_id = g.owner_id AND m.role <> 'guest'
            ORDER BY m.created_at, m.workspace_id LIMIT 1)
         WHERE g.workspace_id IS NULL",
    )
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}

/// Graphs that belong to a team: live ones, and deleted ones, which keep
/// their runs' evidence (and the team that may read it) until the retention purge.
pub async fn count_in_team(
    db: impl PgExecutor<'_>,
    team_id: Uuid,
) -> Result<(i64, i64), sqlx::Error> {
    sqlx::query_as(
        "SELECT count(*) FILTER (WHERE deleted_at IS NULL), count(*) FILTER (WHERE deleted_at IS NOT NULL)
         FROM graphs WHERE team_id = $1",
    )
    .bind(team_id)
    .fetch_one(db)
    .await
}

/// Summary of one graph (for realtime `graph.updated`).
pub async fn summary(
    db: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<GraphSummary>, sqlx::Error> {
    sqlx::query_as(
        "SELECT g.id, g.workspace_id, g.team_id, g.name, g.description, g.updated_at,
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
    sqlx::query_as(concat!(
        "UPDATE graphs SET name = COALESCE($3, name), description = COALESCE($4, description),
                goal = COALESCE($5, goal), version = version + 1, updated_at = now()
         WHERE id = $1 AND ",
        graph_access!("graphs", "$2"),
        " RETURNING id, owner_id, workspace_id, team_id, name, description, goal, version, created_at, updated_at"
    ))
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

/// Deletes a graph that `user` may work on by archiving it: it is marked
/// deleted by `user`, every reader of graphs skips it from now on, and its
/// runs with their evidence stay until the retention purge removes the graph
/// for good (`repo::evidence`). Returns its name and workspace; `None` when
/// absent or already deleted.
pub async fn archive(
    db: impl PgExecutor<'_>,
    user: Uuid,
    id: Uuid,
) -> Result<Option<(String, Option<Uuid>)>, sqlx::Error> {
    sqlx::query_as(concat!(
        "UPDATE graphs SET deleted_at = now(), deleted_by = $2 WHERE id = $1 AND ",
        graph_access!("graphs", "$2"),
        " RETURNING name, workspace_id"
    ))
    .bind(id)
    .bind(user)
    .fetch_optional(db)
    .await
}

/// Archives the graphs an agent's work is recorded in (an external agent's
/// or an exposed agent's managed graph) before the agent is removed, and
/// unlinks them from it, so that removing the agent leaves its runs'
/// evidence in place. Returns the graphs `(id, name, workspace)` archived now.
pub async fn archive_managed_by(
    conn: &mut sqlx::PgConnection,
    agent_id: Uuid,
    user: Uuid,
) -> Result<Vec<(Uuid, String, Option<Uuid>)>, sqlx::Error> {
    let archived = sqlx::query_as(
        "UPDATE graphs SET deleted_at = now(), deleted_by = $2
         WHERE (external_agent_id = $1 OR a2a_agent_id = $1) AND deleted_at IS NULL
         RETURNING id, name, workspace_id",
    )
    .bind(agent_id)
    .bind(user)
    .fetch_all(&mut *conn)
    .await?;
    sqlx::query("UPDATE graphs SET external_agent_id = NULL WHERE external_agent_id = $1")
        .bind(agent_id)
        .execute(&mut *conn)
        .await?;
    sqlx::query("UPDATE graphs SET a2a_agent_id = NULL WHERE a2a_agent_id = $1")
        .bind(agent_id)
        .execute(&mut *conn)
        .await?;
    Ok(archived)
}

/// Whether the graph was deleted (archived); false when there is no such graph.
pub async fn is_deleted(db: impl PgExecutor<'_>, id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM graphs WHERE id = $1 AND deleted_at IS NOT NULL)",
    )
    .bind(id)
    .fetch_one(db)
    .await
}

/// Whether `user` may read the evidence of the graph's runs: they may open
/// the graph, deleted or not (`graph_access_any!`).
pub async fn evidence_readable(
    db: impl PgExecutor<'_>,
    user: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(concat!(
        "SELECT EXISTS (SELECT 1 FROM graphs WHERE id = $1 AND ",
        graph_access_any!("graphs", "$2"),
        ")"
    ))
    .bind(id)
    .bind(user)
    .fetch_one(db)
    .await
}

/// The workspace a graph belongs to (no access check; engine use only).
pub async fn workspace_of(db: impl PgExecutor<'_>, id: Uuid) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT workspace_id FROM graphs WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
        .map(Option::flatten)
}
