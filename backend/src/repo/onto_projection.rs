//! The rows the workspace projection is built from (names, states and ids,
//! never content), and what it put into onto-intelli so far.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{FromRow, PgExecutor};
use uuid::Uuid;

#[derive(Debug, Clone, FromRow)]
pub struct TeamRow {
    pub id: Uuid,
    pub name: String,
    pub key: String,
    pub private: bool,
}

#[derive(Debug, Clone, FromRow)]
pub struct ProjectRow {
    pub id: Uuid,
    pub name: String,
    pub status: String,
    pub lead_id: Option<Uuid>,
}

#[derive(Debug, Clone, FromRow)]
pub struct CycleRow {
    pub id: Uuid,
    pub team_id: Uuid,
    pub number: i32,
    pub name: String,
    pub starts_on: NaiveDate,
    pub ends_on: NaiveDate,
    pub private: bool,
}

#[derive(Debug, Clone, FromRow)]
pub struct IssueRow {
    pub id: Uuid,
    pub team_id: Uuid,
    pub team_key: String,
    pub number: i32,
    pub title: String,
    pub state_category: String,
    pub state_name: String,
    pub priority: i16,
    pub assignee_id: Option<Uuid>,
    /// The principal of the agent the issue is delegated to.
    pub agent_principal: Option<String>,
    pub project_id: Option<Uuid>,
    pub graph_id: Option<Uuid>,
    pub cycle_id: Option<Uuid>,
    pub parent_id: Option<Uuid>,
    pub creator_id: Option<Uuid>,
    pub private: bool,
}

#[derive(Debug, Clone, FromRow)]
pub struct GraphRow {
    pub id: Uuid,
    pub name: String,
    pub team_id: Option<Uuid>,
    pub owner_id: Uuid,
    pub private: bool,
}

#[derive(Debug, Clone, FromRow)]
pub struct DocumentRow {
    pub id: Uuid,
    pub name: String,
    pub status: String,
    pub label: String,
    pub uploaded_by: Option<Uuid>,
}

#[derive(Debug, Clone, FromRow)]
pub struct PersonRow {
    pub id: Uuid,
    pub name: String,
    pub role: String,
    /// The teams of the workspace the person is in.
    pub teams: Vec<Uuid>,
}

#[derive(Debug, Clone, FromRow)]
pub struct RunRow {
    pub id: Uuid,
    pub graph_id: Uuid,
    pub owner_id: Uuid,
    pub status: String,
    pub started_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, FromRow)]
pub struct AgentRow {
    pub id: Uuid,
    pub principal_id: String,
    pub team_id: Option<Uuid>,
}

pub async fn teams(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<TeamRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, name, key, private FROM teams WHERE workspace_id = $1 AND id = ANY($2)",
    )
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(db)
    .await
}

pub async fn projects(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<ProjectRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, name, status, lead_id FROM projects WHERE workspace_id = $1 AND id = ANY($2)",
    )
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(db)
    .await
}

pub async fn cycles(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<CycleRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT c.id, c.team_id, c.number, c.name, c.starts_on, c.ends_on, t.private
         FROM cycles c JOIN teams t ON t.id = c.team_id
         WHERE t.workspace_id = $1 AND c.id = ANY($2)",
    )
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(db)
    .await
}

pub async fn issues(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<IssueRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT i.id, i.team_id, t.key AS team_key, i.number, i.title,
                s.category AS state_category, s.name AS state_name, i.priority,
                i.assignee_id, a.principal_id AS agent_principal, i.project_id, i.graph_id,
                i.cycle_id, i.parent_id, i.creator_id, t.private
         FROM issues i JOIN teams t ON t.id = i.team_id
         JOIN issue_states s ON s.id = i.state_id
         LEFT JOIN agents a ON a.id = i.agent_id
         WHERE i.workspace_id = $1 AND i.id = ANY($2)",
    )
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(db)
    .await
}

pub async fn graphs(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<GraphRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT g.id, g.name, g.team_id, g.owner_id, COALESCE(t.private, false) AS private
         FROM graphs g LEFT JOIN teams t ON t.id = g.team_id
         WHERE g.workspace_id = $1 AND g.id = ANY($2)",
    )
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(db)
    .await
}

pub async fn documents(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<DocumentRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, name, status, label, uploaded_by FROM documents
         WHERE workspace_id = $1 AND id = ANY($2)",
    )
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(db)
    .await
}

pub async fn people(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<PersonRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT u.id, u.name, m.role,
                ARRAY(SELECT tm.team_id FROM team_members tm JOIN teams t ON t.id = tm.team_id
                       WHERE tm.user_id = u.id AND t.workspace_id = $1 ORDER BY tm.team_id) AS teams
         FROM workspace_members m JOIN users u ON u.id = m.user_id
         WHERE m.workspace_id = $1 AND u.id = ANY($2)",
    )
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(db)
    .await
}

pub async fn runs(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<RunRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT r.id, r.graph_id, r.owner_id, r.status, r.started_at
         FROM runs r JOIN graphs g ON g.id = r.graph_id
         WHERE g.workspace_id = $1 AND r.id = ANY($2)",
    )
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(db)
    .await
}

pub async fn agents(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<AgentRow>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, principal_id, team_id FROM agents WHERE workspace_id = $1 AND id = ANY($2)",
    )
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(db)
    .await
}

/// A page of the ids of one kind of a workspace, in id order, after `after`
/// (the backfill's walk). `kind` is one of the projection's kinds.
pub async fn ids(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    kind: &str,
    after: Option<Uuid>,
    limit: i64,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let statement = match kind {
        "team" => {
            "SELECT id FROM teams WHERE workspace_id = $1 AND ($2::uuid IS NULL OR id > $2) ORDER BY id LIMIT $3"
        }
        "project" => {
            "SELECT id FROM projects WHERE workspace_id = $1 AND ($2::uuid IS NULL OR id > $2) ORDER BY id LIMIT $3"
        }
        "cycle" => {
            "SELECT c.id FROM cycles c JOIN teams t ON t.id = c.team_id WHERE t.workspace_id = $1 AND ($2::uuid IS NULL OR c.id > $2) ORDER BY c.id LIMIT $3"
        }
        "issue" => {
            "SELECT id FROM issues WHERE workspace_id = $1 AND ($2::uuid IS NULL OR id > $2) ORDER BY id LIMIT $3"
        }
        "graph" => {
            "SELECT id FROM graphs WHERE workspace_id = $1 AND ($2::uuid IS NULL OR id > $2) ORDER BY id LIMIT $3"
        }
        "document" => {
            "SELECT id FROM documents WHERE workspace_id = $1 AND ($2::uuid IS NULL OR id > $2) ORDER BY id LIMIT $3"
        }
        "person" => {
            "SELECT user_id FROM workspace_members WHERE workspace_id = $1 AND ($2::uuid IS NULL OR user_id > $2) ORDER BY user_id LIMIT $3"
        }
        "run_started" => {
            "SELECT r.id FROM runs r JOIN graphs g ON g.id = r.graph_id WHERE g.workspace_id = $1 AND ($2::uuid IS NULL OR r.id > $2) ORDER BY r.id LIMIT $3"
        }
        "agent" => {
            "SELECT id FROM agents WHERE workspace_id = $1 AND ($2::uuid IS NULL OR id > $2) ORDER BY id LIMIT $3"
        }
        _ => return Ok(Vec::new()),
    };
    sqlx::query_scalar(statement)
        .bind(workspace_id)
        .bind(after)
        .bind(limit)
        .fetch_all(db)
        .await
}

/// The links the projection stated from each of `entities`, for those it
/// put into onto-intelli.
pub async fn tracked(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    entities: &[String],
) -> Result<HashMap<String, Vec<String>>, sqlx::Error> {
    let rows: Vec<(String, Vec<String>)> = sqlx::query_as(
        "SELECT entity, links FROM onto_projection WHERE workspace_id = $1 AND entity = ANY($2)",
    )
    .bind(workspace_id)
    .bind(entities)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Records what onto-intelli now holds: `entities` with the links stated from them.
pub async fn record(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    entities: &[(String, Vec<String>)],
) -> Result<(), sqlx::Error> {
    if entities.is_empty() {
        return Ok(());
    }
    let names: Vec<&str> = entities.iter().map(|(e, _)| e.as_str()).collect();
    // A text[][] cannot be ragged, so the links travel as one JSON array per entity.
    let links: Vec<serde_json::Value> =
        entities.iter().map(|(_, l)| serde_json::json!(l)).collect();
    sqlx::query(
        "INSERT INTO onto_projection (workspace_id, entity, links)
         SELECT $1, e.entity, ARRAY(SELECT jsonb_array_elements_text(e.links))
         FROM unnest($2::text[], $3::jsonb[]) AS e(entity, links)
         ON CONFLICT (workspace_id, entity) DO UPDATE SET links = EXCLUDED.links, updated_at = now()",
    )
    .bind(workspace_id)
    .bind(&names)
    .bind(&links)
    .execute(db)
    .await?;
    Ok(())
}

/// Forgets entities deleted from onto-intelli.
pub async fn forget(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    entities: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM onto_projection WHERE workspace_id = $1 AND entity = ANY($2)")
        .bind(workspace_id)
        .bind(entities)
        .execute(db)
        .await?;
    Ok(())
}

/// Records that the workspace was projected whole.
pub async fn mark_projected(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE onto_export_cursors SET projected_at = now(), updated_at = now() WHERE workspace_id = $1",
    )
    .bind(workspace_id)
    .execute(db)
    .await?;
    Ok(())
}
