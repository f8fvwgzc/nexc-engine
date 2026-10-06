//! Issues, workflow states and projects. Reads join everything an issue
//! shows (team key, state, assignee) in one statement.

use chrono::NaiveDate;
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgConnection, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::issue::{
    Issue, IssuePerson, IssueState, Project, STARTER_STATES, StateCategory,
};

impl FromRow<'_, PgRow> for IssueState {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(IssueState {
            id: row.try_get("id")?,
            team_id: row.try_get("team_id")?,
            name: row.try_get("name")?,
            category: enum_col(row, "category")?,
            color: row.try_get("color")?,
            position: row.try_get("position")?,
        })
    }
}

impl FromRow<'_, PgRow> for Issue {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let team_id = row.try_get("team_id")?;
        let number: i32 = row.try_get("number")?;
        let team_key: String = row.try_get("team_key")?;
        let assignee_id: Option<Uuid> = row.try_get("assignee_id")?;
        let assignee_name: Option<String> = row.try_get("assignee_name")?;
        Ok(Issue {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            team_id,
            identifier: format!("{team_key}-{number}"),
            number,
            title: row.try_get("title")?,
            description: row.try_get("description")?,
            state: IssueState {
                id: row.try_get("state_id")?,
                team_id,
                name: row.try_get("state_name")?,
                category: enum_col(row, "state_category")?,
                color: row.try_get("state_color")?,
                position: row.try_get("state_position")?,
            },
            priority: row.try_get("priority")?,
            assignee: assignee_id.map(|user_id| IssuePerson {
                user_id,
                name: assignee_name.unwrap_or_default(),
            }),
            agent_id: row.try_get("agent_id")?,
            project_id: row.try_get("project_id")?,
            graph_id: row.try_get("graph_id")?,
            creator_id: row.try_get("creator_id")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            completed_at: row.try_get("completed_at")?,
        })
    }
}

impl FromRow<'_, PgRow> for Project {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Project {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            status: enum_col(row, "status")?,
            lead_id: row.try_get("lead_id")?,
            target_date: row.try_get("target_date")?,
            issue_count: row.try_get("issue_count")?,
            closed_count: row.try_get("closed_count")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// An issue with its team key, state and assignee, for user `$1` who must be
/// a member of the issue's workspace and able to see its team.
macro_rules! issue_for_user {
    ($tail:literal) => {
        concat!(
            "SELECT i.id, i.workspace_id, i.team_id, t.key AS team_key, i.number, i.title,
                    i.description, i.priority, i.assignee_id, au.name AS assignee_name, i.agent_id,
                    i.project_id, i.graph_id, i.creator_id, i.created_at, i.updated_at,
                    i.completed_at, s.id AS state_id, s.name AS state_name,
                    s.category AS state_category, s.color AS state_color,
                    s.position AS state_position
             FROM issues i
             JOIN teams t ON t.id = i.team_id
             JOIN issue_states s ON s.id = i.state_id
             JOIN workspace_members wm ON wm.workspace_id = i.workspace_id AND wm.user_id = $1
             LEFT JOIN users au ON au.id = i.assignee_id
             WHERE ",
            team_visible!("t", "wm", "$1"),
            " AND ",
            $tail
        )
    };
}

// ---------- workflow states ----------

/// The workflow of a team, in order.
pub async fn states(
    db: impl PgExecutor<'_>,
    team_id: Uuid,
) -> Result<Vec<IssueState>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM issue_states WHERE team_id = $1 ORDER BY position, name")
        .bind(team_id)
        .fetch_all(db)
        .await
}

/// One state of a team.
pub async fn find_state(
    db: impl PgExecutor<'_>,
    team_id: Uuid,
    id: Uuid,
) -> Result<Option<IssueState>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM issue_states WHERE team_id = $1 AND id = $2")
        .bind(team_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Adds a state at the end of its category's place in the workflow.
pub async fn create_state(
    db: impl PgExecutor<'_>,
    team_id: Uuid,
    name: &str,
    category: StateCategory,
    color: &str,
    position: i32,
) -> Result<IssueState, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO issue_states (id, team_id, name, category, color, position)
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(team_id)
    .bind(name)
    .bind(category.as_str())
    .bind(color)
    .bind(position)
    .fetch_one(db)
    .await
}

/// Writes every field of a state.
pub async fn save_state(
    db: impl PgExecutor<'_>,
    s: &IssueState,
) -> Result<IssueState, sqlx::Error> {
    sqlx::query_as(
        "UPDATE issue_states SET name = $3, category = $4, color = $5, position = $6
         WHERE team_id = $1 AND id = $2 RETURNING *",
    )
    .bind(s.team_id)
    .bind(s.id)
    .bind(&s.name)
    .bind(s.category.as_str())
    .bind(&s.color)
    .bind(s.position)
    .fetch_one(db)
    .await
}

/// Number of issues in a state.
pub async fn count_in_state(db: impl PgExecutor<'_>, state_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM issues WHERE state_id = $1")
        .bind(state_id)
        .fetch_one(db)
        .await
}

/// Deletes a state that no issue is in.
pub async fn delete_state(
    db: impl PgExecutor<'_>,
    team_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM issue_states WHERE team_id = $1 AND id = $2")
        .bind(team_id)
        .bind(id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// Gives a team the starter workflow.
pub async fn seed_states(db: &mut PgConnection, team_id: Uuid) -> Result<(), sqlx::Error> {
    for (position, (name, category, color)) in STARTER_STATES.into_iter().enumerate() {
        create_state(&mut *db, team_id, name, category, color, position as i32).await?;
    }
    Ok(())
}

/// Teams that have no workflow yet (created before issues existed).
pub async fn teams_without_states(db: impl PgExecutor<'_>) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT t.id FROM teams t
         WHERE NOT EXISTS (SELECT 1 FROM issue_states s WHERE s.team_id = t.id)",
    )
    .fetch_all(db)
    .await
}

/// The state a new issue starts in: the first "unstarted" one, else the
/// first of the workflow.
pub async fn default_state(
    db: impl PgExecutor<'_>,
    team_id: Uuid,
) -> Result<Option<IssueState>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM issue_states WHERE team_id = $1
         ORDER BY (category <> 'unstarted'), position, name LIMIT 1",
    )
    .bind(team_id)
    .fetch_optional(db)
    .await
}

// ---------- issues ----------

/// What narrows an issue list; `None` leaves a dimension open.
#[derive(Debug, Clone, Default)]
pub struct IssueFilter {
    pub team_id: Option<Uuid>,
    pub assignee_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    /// Only issues that still need work (not completed or canceled).
    pub open_only: bool,
    /// Matches the title or the identifier, case-insensitively.
    pub q: Option<String>,
    pub limit: i64,
}

/// Issues of a workspace that `user_id` can see, most recently updated first.
pub async fn list(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    f: &IssueFilter,
) -> Result<Vec<Issue>, sqlx::Error> {
    sqlx::query_as(issue_for_user!(
        "i.workspace_id = $2
         AND ($3::uuid IS NULL OR i.team_id = $3)
         AND ($4::uuid IS NULL OR i.assignee_id = $4)
         AND ($5::uuid IS NULL OR i.project_id = $5)
         AND (NOT $6 OR s.category NOT IN ('completed', 'canceled'))
         AND ($7::text IS NULL OR i.title ILIKE '%' || $7 || '%'
              OR (t.key || '-' || i.number) ILIKE $7 || '%')
         ORDER BY i.updated_at DESC, i.id LIMIT $8"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .bind(f.team_id)
    .bind(f.assignee_id)
    .bind(f.project_id)
    .bind(f.open_only)
    .bind(f.q.as_deref())
    .bind(f.limit)
    .fetch_all(db)
    .await
}

/// One issue, if `user_id` can see it.
pub async fn find(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    id: Uuid,
) -> Result<Option<Issue>, sqlx::Error> {
    sqlx::query_as(issue_for_user!("i.id = $2"))
        .bind(user_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Fields of an issue to insert.
#[derive(Debug, Clone)]
pub struct NewIssue<'a> {
    pub workspace_id: Uuid,
    pub team_id: Uuid,
    pub title: &'a str,
    pub description: &'a str,
    pub state_id: Uuid,
    pub closed: bool,
    pub priority: i16,
    pub assignee_id: Option<Uuid>,
    pub agent_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
    pub creator_id: Uuid,
}

/// Inserts an issue with the team's next number. Must run in a transaction:
/// the counter row is locked until the issue exists.
pub async fn create(db: &mut PgConnection, n: &NewIssue<'_>) -> Result<Uuid, sqlx::Error> {
    let number: i32 = sqlx::query_scalar(
        "UPDATE teams SET next_issue_number = next_issue_number + 1 WHERE id = $1
         RETURNING next_issue_number - 1",
    )
    .bind(n.team_id)
    .fetch_one(&mut *db)
    .await?;
    sqlx::query_scalar(
        "INSERT INTO issues (id, workspace_id, team_id, number, title, description, state_id, priority,
                             assignee_id, agent_id, project_id, creator_id, completed_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                 CASE WHEN $13 THEN now() END)
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(n.workspace_id)
    .bind(n.team_id)
    .bind(number)
    .bind(n.title)
    .bind(n.description)
    .bind(n.state_id)
    .bind(n.priority)
    .bind(n.assignee_id)
    .bind(n.agent_id)
    .bind(n.project_id)
    .bind(n.creator_id)
    .bind(n.closed)
    .fetch_one(&mut *db)
    .await
}

/// Writes the editable fields of an issue. `completed_at` follows the state:
/// set when the issue enters a closed state, cleared when it is reopened.
pub async fn save(db: impl PgExecutor<'_>, issue: &Issue) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE issues SET title = $2, description = $3, state_id = $4, priority = $5,
                assignee_id = $6, agent_id = $7, project_id = $8, updated_at = now(),
                completed_at = CASE WHEN NOT $9 THEN NULL
                                    ELSE COALESCE(completed_at, now()) END
         WHERE id = $1",
    )
    .bind(issue.id)
    .bind(&issue.title)
    .bind(&issue.description)
    .bind(issue.state.id)
    .bind(issue.priority)
    .bind(issue.assignee.as_ref().map(|a| a.user_id))
    .bind(issue.agent_id)
    .bind(issue.project_id)
    .bind(issue.state.category.is_closed())
    .execute(db)
    .await?;
    Ok(())
}

/// Links an issue to the graph that plans and executes it.
pub async fn link_graph(
    db: impl PgExecutor<'_>,
    id: Uuid,
    graph_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE issues SET graph_id = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(graph_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Deletes an issue.
pub async fn delete(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM issues WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

// ---------- projects ----------

/// A project with the counts of its issues that user `$1` can see.
macro_rules! project_for_user {
    ($tail:literal) => {
        concat!(
            "SELECT p.id, p.workspace_id, p.name, p.description, p.status, p.lead_id, p.target_date,
                    p.created_at, COALESCE(c.issue_count, 0) AS issue_count,
                    COALESCE(c.closed_count, 0) AS closed_count
             FROM projects p
             LEFT JOIN LATERAL (
                SELECT count(*) AS issue_count,
                       count(*) FILTER (WHERE s.category IN ('completed', 'canceled')) AS closed_count
                FROM issues i
                JOIN teams t ON t.id = i.team_id
                JOIN issue_states s ON s.id = i.state_id
                JOIN workspace_members wm ON wm.workspace_id = i.workspace_id AND wm.user_id = $1
                WHERE i.project_id = p.id AND ",
            team_visible!("t", "wm", "$1"),
            ") c ON true WHERE ",
            $tail
        )
    };
}

/// Projects of a workspace, oldest first.
pub async fn projects(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
) -> Result<Vec<Project>, sqlx::Error> {
    sqlx::query_as(project_for_user!(
        "p.workspace_id = $2 ORDER BY p.created_at, p.id"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// One project of a workspace.
pub async fn find_project(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<Project>, sqlx::Error> {
    sqlx::query_as(project_for_user!("p.workspace_id = $2 AND p.id = $3"))
        .bind(user_id)
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Inserts a project.
pub async fn create_project(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    created_by: Uuid,
    name: &str,
    description: &str,
    target_date: Option<NaiveDate>,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO projects (id, workspace_id, name, description, lead_id, target_date, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $5) RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(workspace_id)
    .bind(name)
    .bind(description)
    .bind(created_by)
    .bind(target_date)
    .fetch_one(db)
    .await
}

/// Writes the editable fields of a project.
pub async fn save_project(db: impl PgExecutor<'_>, p: &Project) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE projects SET name = $2, description = $3, status = $4, lead_id = $5,
                target_date = $6, updated_at = now() WHERE id = $1",
    )
    .bind(p.id)
    .bind(&p.name)
    .bind(&p.description)
    .bind(p.status.as_str())
    .bind(p.lead_id)
    .bind(p.target_date)
    .execute(db)
    .await?;
    Ok(())
}

/// Deletes a project; its issues stay, without a project.
pub async fn delete_project(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM projects WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Whether a project belongs to a workspace.
pub async fn project_in_workspace(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM projects WHERE workspace_id = $1 AND id = $2)")
        .bind(workspace_id)
        .bind(id)
        .fetch_one(db)
        .await
}
