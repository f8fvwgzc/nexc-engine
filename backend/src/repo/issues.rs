//! Issues, workflow states and projects. Reads join everything an issue
//! shows (team key, state, assignee) in one statement.

use chrono::NaiveDate;
use sqlx::postgres::PgRow;
use sqlx::types::Json;
use sqlx::{FromRow, PgConnection, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::cycle::CycleRef;
use crate::domain::issue::{
    Issue, IssueChange, IssueEvent, IssuePerson, IssueRef, IssueState, Label, NodeOutcome,
    Notification, NotificationAlert, NotificationApproval, NotificationIssue, NotificationKind,
    Project, ProjectStatus, RUN_REPORT_MAX_CHARS, STARTER_STATES, StateCategory, SubIssueCount,
    TeamRef,
};
use crate::domain::run::RunStatus;

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
        let Json(labels): Json<Vec<Label>> = row.try_get("labels")?;
        // Null unless the parent exists and its team is visible to the reader.
        let parent_key: Option<String> = row.try_get("parent_key")?;
        let parent = match parent_key {
            Some(key) => {
                let number: i32 = row.try_get("parent_number")?;
                Some(IssueRef {
                    id: row.try_get("parent_id")?,
                    identifier: format!("{key}-{number}"),
                    title: row.try_get("parent_title")?,
                })
            }
            None => None,
        };
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
            labels,
            parent,
            sub_issues: SubIssueCount {
                total: row.try_get("sub_total")?,
                closed: row.try_get("sub_closed")?,
            },
            cycle: match row.try_get::<Option<Uuid>, _>("cycle_id")? {
                Some(id) => Some(CycleRef {
                    id,
                    number: row.try_get("cycle_number")?,
                    name: row.try_get("cycle_name")?,
                }),
                None => None,
            },
            assignee: assignee_id.map(|user_id| IssuePerson {
                user_id,
                name: assignee_name.unwrap_or_default(),
            }),
            agent_id: row.try_get("agent_id")?,
            project_id: row.try_get("project_id")?,
            graph_id: row.try_get("graph_id")?,
            creator_id: row.try_get("creator_id")?,
            due_date: row.try_get("due_date")?,
            github_url: row.try_get("github_url")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
            completed_at: row.try_get("completed_at")?,
        })
    }
}

impl FromRow<'_, PgRow> for Project {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let team_id: Option<Uuid> = row.try_get("team_id")?;
        let team = match team_id {
            Some(id) => Some(TeamRef {
                id,
                key: row.try_get("team_key")?,
                name: row.try_get("team_name")?,
            }),
            None => None,
        };
        Ok(Project {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            status: enum_col(row, "status")?,
            team_id,
            team,
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
                    i.project_id, i.graph_id, i.creator_id, i.due_date, i.github_url, i.created_at,
                    i.updated_at, i.completed_at, s.id AS state_id, s.name AS state_name,
                    s.category AS state_category, s.color AS state_color,
                    s.position AS state_position,
                    COALESCE((SELECT jsonb_agg(jsonb_build_object(
                                         'id', l.id, 'name', l.name, 'color', l.color)
                                     ORDER BY lower(l.name))
                              FROM issue_labels il JOIN labels l ON l.id = il.label_id
                              WHERE il.issue_id = i.id), '[]'::jsonb) AS labels,
                    p.id AS parent_id, pt.key AS parent_key, p.number AS parent_number,
                    p.title AS parent_title,
                    i.cycle_id, cy.number AS cycle_number, cy.name AS cycle_name,
                    (SELECT count(*) FROM issues c WHERE c.parent_id = i.id) AS sub_total,
                    (SELECT count(*) FROM issues c JOIN issue_states cs ON cs.id = c.state_id
                     WHERE c.parent_id = i.id AND cs.category IN ('completed', 'canceled'))
                        AS sub_closed
             FROM issues i
             JOIN teams t ON t.id = i.team_id
             JOIN issue_states s ON s.id = i.state_id
             JOIN workspace_members wm ON wm.workspace_id = i.workspace_id AND wm.user_id = $1
             LEFT JOIN users au ON au.id = i.assignee_id
             LEFT JOIN cycles cy ON cy.id = i.cycle_id
             LEFT JOIN issues p ON p.id = i.parent_id
             LEFT JOIN teams pt ON pt.id = p.team_id AND ",
            team_visible!("pt", "wm", "$1"),
            "
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
    /// Only issues that carry this label.
    pub label_id: Option<Uuid>,
    /// Only the sub-issues of this issue.
    pub parent_id: Option<Uuid>,
    /// Only issues planned in this cycle.
    pub cycle_id: Option<Uuid>,
    /// Only issues this person filed.
    pub creator_id: Option<Uuid>,
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
         AND ($9::uuid IS NULL OR EXISTS (
              SELECT 1 FROM issue_labels fl WHERE fl.issue_id = i.id AND fl.label_id = $9))
         AND ($10::uuid IS NULL OR i.parent_id = $10)
         AND ($11::uuid IS NULL OR i.cycle_id = $11)
         AND ($12::uuid IS NULL OR i.creator_id = $12)
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
    .bind(f.label_id)
    .bind(f.parent_id)
    .bind(f.cycle_id)
    .bind(f.creator_id)
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
    /// The canonical link to a GitHub issue or pull request ([`crate::domain::issue::github_url`]).
    pub github_url: Option<&'a str>,
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
                             assignee_id, agent_id, project_id, creator_id, completed_at, github_url)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                 CASE WHEN $13 THEN now() END, $14)
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
    .bind(n.github_url)
    .fetch_one(&mut *db)
    .await
}

/// Writes the editable fields of an issue. `completed_at` follows the state:
/// set when the issue enters a closed state, cleared when it is reopened.
pub async fn save(db: impl PgExecutor<'_>, issue: &Issue) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE issues SET title = $2, description = $3, state_id = $4, priority = $5,
                assignee_id = $6, agent_id = $7, project_id = $8, github_url = $10,
                updated_at = now(),
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
    .bind(issue.github_url.as_deref())
    .execute(db)
    .await?;
    Ok(())
}

/// Makes an issue part of another one, or an issue of its own.
pub async fn set_parent(
    db: impl PgExecutor<'_>,
    id: Uuid,
    parent_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE issues SET parent_id = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(parent_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Plans an issue in a cycle, or takes it out of its cycle.
pub async fn set_cycle(
    db: impl PgExecutor<'_>,
    id: Uuid,
    cycle_id: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE issues SET cycle_id = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(cycle_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Sets the day an issue is due, or removes it.
pub async fn set_due(
    db: impl PgExecutor<'_>,
    id: Uuid,
    due_date: Option<chrono::NaiveDate>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE issues SET due_date = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(due_date)
        .execute(db)
        .await?;
    Ok(())
}

/// Whether `candidate` is `id` itself or one of the issues under it, which
/// makes it unusable as `id`'s parent.
pub async fn is_within(
    db: impl PgExecutor<'_>,
    id: Uuid,
    candidate: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "WITH RECURSIVE under AS (
             SELECT $1::uuid AS id
             UNION
             SELECT c.id FROM issues c JOIN under u ON c.parent_id = u.id)
         SELECT EXISTS (SELECT 1 FROM under WHERE id = $2)",
    )
    .bind(id)
    .bind(candidate)
    .fetch_one(db)
    .await
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

// ---------- labels ----------

impl FromRow<'_, PgRow> for Label {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Label {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            color: row.try_get("color")?,
        })
    }
}

/// The labels of a workspace, by name.
pub async fn labels(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<Label>, sqlx::Error> {
    sqlx::query_as(
        "SELECT id, name, color FROM labels WHERE workspace_id = $1 ORDER BY lower(name)",
    )
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// One label of a workspace.
pub async fn find_label(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<Label>, sqlx::Error> {
    sqlx::query_as("SELECT id, name, color FROM labels WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

pub async fn count_labels(db: impl PgExecutor<'_>, workspace_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM labels WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(db)
        .await
}

/// Adds a label. Names are unique in a workspace, ignoring case.
pub async fn create_label(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    name: &str,
    color: &str,
) -> Result<Label, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO labels (id, workspace_id, name, color) VALUES ($1, $2, $3, $4)
         RETURNING id, name, color",
    )
    .bind(Uuid::now_v7())
    .bind(workspace_id)
    .bind(name)
    .bind(color)
    .fetch_one(db)
    .await
}

pub async fn save_label(db: impl PgExecutor<'_>, label: &Label) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE labels SET name = $2, color = $3 WHERE id = $1")
        .bind(label.id)
        .bind(&label.name)
        .bind(&label.color)
        .execute(db)
        .await?;
    Ok(())
}

/// Deletes a label; issues that carried it lose it.
pub async fn delete_label(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM labels WHERE id = $1")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// How many of `ids` are labels of the workspace.
pub async fn count_known_labels(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM labels WHERE workspace_id = $1 AND id = ANY($2)")
        .bind(workspace_id)
        .bind(ids)
        .fetch_one(db)
        .await
}

/// Makes `ids` the labels of an issue.
pub async fn set_labels(
    db: &mut PgConnection,
    issue_id: Uuid,
    ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM issue_labels WHERE issue_id = $1 AND label_id <> ALL($2)")
        .bind(issue_id)
        .bind(ids)
        .execute(&mut *db)
        .await?;
    sqlx::query(
        "INSERT INTO issue_labels (issue_id, label_id) SELECT $1, unnest($2::uuid[])
         ON CONFLICT DO NOTHING",
    )
    .bind(issue_id)
    .bind(ids)
    .execute(&mut *db)
    .await?;
    sqlx::query("UPDATE issues SET updated_at = now() WHERE id = $1")
        .bind(issue_id)
        .execute(&mut *db)
        .await?;
    Ok(())
}

// ---------- timeline ----------

impl FromRow<'_, PgRow> for IssueEvent {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let actor_id: Option<Uuid> = row.try_get("actor_id")?;
        let actor_name: Option<String> = row.try_get("actor_name")?;
        Ok(IssueEvent {
            id: row.try_get("id")?,
            issue_id: row.try_get("issue_id")?,
            kind: enum_col(row, "kind")?,
            actor: actor_id.map(|user_id| IssuePerson {
                user_id,
                name: actor_name.unwrap_or_default(),
            }),
            body: row.try_get("body")?,
            from: row.try_get("from_value")?,
            to: row.try_get("to_value")?,
            created_at: row.try_get("created_at")?,
            edited_at: row.try_get("edited_at")?,
            run_id: row.try_get("run_id")?,
        })
    }
}

macro_rules! event_select {
    ($tail:literal) => {
        concat!(
            "SELECT e.id, e.issue_id, e.kind, e.actor_id, u.name AS actor_name, e.body,
                    e.from_value, e.to_value, e.created_at, e.edited_at, e.run_id
             FROM issue_events e LEFT JOIN users u ON u.id = e.actor_id
             WHERE ",
            $tail
        )
    };
}

/// The timeline of an issue, oldest first.
pub async fn events(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
) -> Result<Vec<IssueEvent>, sqlx::Error> {
    sqlx::query_as(event_select!("e.issue_id = $1 ORDER BY e.created_at, e.id"))
        .bind(issue_id)
        .fetch_all(db)
        .await
}

/// One comment of an issue.
pub async fn find_comment(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    id: Uuid,
) -> Result<Option<IssueEvent>, sqlx::Error> {
    sqlx::query_as(event_select!(
        "e.issue_id = $1 AND e.id = $2 AND e.kind = 'comment'"
    ))
    .bind(issue_id)
    .bind(id)
    .fetch_optional(db)
    .await
}

/// Adds a comment and returns its id.
pub async fn add_comment(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    actor_id: Uuid,
    body: &str,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO issue_events (id, issue_id, actor_id, kind, body)
         VALUES ($1, $2, $3, 'comment', $4) RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(issue_id)
    .bind(actor_id)
    .bind(body)
    .fetch_one(db)
    .await
}

/// Replaces the text of a comment.
pub async fn edit_comment(
    db: impl PgExecutor<'_>,
    id: Uuid,
    body: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE issue_events SET body = $2, edited_at = now() WHERE id = $1")
        .bind(id)
        .bind(body)
        .execute(db)
        .await?;
    Ok(())
}

/// Deletes a comment.
pub async fn delete_comment(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM issue_events WHERE id = $1 AND kind = 'comment'")
        .bind(id)
        .execute(db)
        .await?;
    Ok(())
}

/// Records changes to an issue in its timeline.
pub async fn record_changes(
    db: &mut PgConnection,
    issue_id: Uuid,
    actor_id: Uuid,
    changes: &[IssueChange],
) -> Result<(), sqlx::Error> {
    for change in changes {
        sqlx::query(
            "INSERT INTO issue_events (id, issue_id, actor_id, kind, from_value, to_value)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::now_v7())
        .bind(issue_id)
        .bind(actor_id)
        .bind(change.kind.as_str())
        .bind(&change.from)
        .bind(&change.to)
        .execute(&mut *db)
        .await?;
    }
    Ok(())
}

/// Records what happened to one of an issue's files in its timeline: `name`
/// is the file's, `from` the version it replaced and `to` what was done
/// (`uploaded`, `updated:v<N>`, `duplicated`, `deleted`).
pub async fn record_artifact(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    actor_id: Uuid,
    name: &str,
    from: Option<&str>,
    to: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO issue_events (id, issue_id, actor_id, kind, body, from_value, to_value)
         VALUES ($1, $2, $3, 'artifact', $4, $5, $6)",
    )
    .bind(Uuid::now_v7())
    .bind(issue_id)
    .bind(actor_id)
    .bind(name)
    .bind(from)
    .bind(to)
    .execute(db)
    .await?;
    Ok(())
}

/// Whether any issue hands its work to the graph.
pub async fn any_of_graph(db: impl PgExecutor<'_>, graph_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM issues WHERE graph_id = $1)")
        .bind(graph_id)
        .fetch_one(db)
        .await
}

/// What a finished run tells the issues its graph executes.
#[derive(Debug, Clone)]
pub struct RunReport<'a> {
    pub run_id: Uuid,
    pub graph_id: Uuid,
    /// Who started the run; they hear about it too.
    pub owner_id: Uuid,
    pub status: RunStatus,
    /// The entry's text ([`crate::domain::issue::run_report`]).
    pub body: &'a str,
}

/// What a finished run's nodes left, read for its report: the outputs of
/// the nodes nothing else in the run waited for (the deliverables), in run
/// order, and the nodes that failed with their error.
pub async fn run_outcomes(
    db: &mut PgConnection,
    run_id: Uuid,
) -> Result<(Vec<NodeOutcome>, Vec<NodeOutcome>), sqlx::Error> {
    let rows: Vec<(String, String, bool)> = sqlx::query_as(
        "SELECT n.title,
                CASE WHEN nr.status = 'failed' THEN COALESCE(nr.error, '') ELSE COALESCE(left(nr.output, $2), '') END,
                nr.status = 'failed'
         FROM node_runs nr JOIN nodes n ON n.id = nr.node_id
         WHERE nr.run_id = $1
           AND (nr.status = 'failed'
                OR (nr.status = 'succeeded' AND NOT EXISTS (
                    SELECT 1 FROM edges e JOIN node_runs d ON d.run_id = nr.run_id AND d.node_id = e.target
                    WHERE e.source = nr.node_id AND e.blocking)))
         ORDER BY nr.position",
    )
    .bind(run_id)
    // Enough for the report to tell when it was cut, without reading whole documents.
    .bind((RUN_REPORT_MAX_CHARS * 2) as i32)
    .fetch_all(&mut *db)
    .await?;
    let (failed, done): (Vec<_>, Vec<_>) = rows.into_iter().partition(|(_, _, failed)| *failed);
    let outcome = |(title, text, _): (String, String, bool)| NodeOutcome { title, text };
    Ok((
        done.into_iter().map(outcome).collect(),
        failed.into_iter().map(outcome).collect(),
    ))
}

/// Writes a finished run onto every issue its graph executes, in the run's
/// own (fenced) transaction so it happens once: a `run` entry in the
/// timeline with the run's id and status; when the run succeeded and the
/// issue is still open, a move to the team's review state (its last
/// `started` state, "In Review" in the starter workflow), recorded as a
/// state change nobody made; and an inbox entry for the assignee, the
/// creator and whoever started the run. Returns the issues reported to.
pub async fn report_run(
    db: &mut PgConnection,
    report: &RunReport<'_>,
) -> Result<Vec<Uuid>, sqlx::Error> {
    let issues: Vec<(Uuid, Uuid, String, String)> = sqlx::query_as(
        "SELECT i.id, i.team_id, s.name, s.category
         FROM issues i JOIN issue_states s ON s.id = i.state_id
         WHERE i.graph_id = $1
         ORDER BY i.id
         FOR UPDATE OF i",
    )
    .bind(report.graph_id)
    .fetch_all(&mut *db)
    .await?;
    let mut reported = Vec::with_capacity(issues.len());
    for (issue_id, team_id, state_name, category) in issues {
        sqlx::query(
            "INSERT INTO issue_events (id, issue_id, actor_id, kind, body, to_value, run_id)
             VALUES ($1, $2, NULL, 'run', $3, $4, $5)",
        )
        .bind(Uuid::now_v7())
        .bind(issue_id)
        .bind(report.body)
        .bind(report.status.as_str())
        .bind(report.run_id)
        .execute(&mut *db)
        .await?;
        let open = !matches!(category.as_str(), "completed" | "canceled");
        if report.status == RunStatus::Succeeded && open {
            let review: Option<(Uuid, String)> = sqlx::query_as(
                "SELECT id, name FROM issue_states WHERE team_id = $1 AND category = 'started'
                 ORDER BY position DESC, name DESC LIMIT 1",
            )
            .bind(team_id)
            .fetch_optional(&mut *db)
            .await?;
            if let Some((review_id, review_name)) = review.filter(|(_, name)| *name != state_name) {
                sqlx::query("UPDATE issues SET state_id = $2 WHERE id = $1")
                    .bind(issue_id)
                    .bind(review_id)
                    .execute(&mut *db)
                    .await?;
                sqlx::query(
                    "INSERT INTO issue_events (id, issue_id, actor_id, kind, from_value, to_value)
                     VALUES ($1, $2, NULL, 'state', $3, $4)",
                )
                .bind(Uuid::now_v7())
                .bind(issue_id)
                .bind(&state_name)
                .bind(&review_name)
                .execute(&mut *db)
                .await?;
            }
        }
        sqlx::query("UPDATE issues SET updated_at = now() WHERE id = $1")
            .bind(issue_id)
            .execute(&mut *db)
            .await?;
        sqlx::query(
            "INSERT INTO notifications (id, user_id, workspace_id, issue_id, actor_id, kind)
             SELECT gen_random_uuid(), r.user_id, i.workspace_id, i.id, NULL, 'run'
             FROM issues i
             CROSS JOIN LATERAL (
                 SELECT DISTINCT x AS user_id FROM unnest(ARRAY[i.assignee_id, i.creator_id, $2]) x
                 WHERE x IS NOT NULL) r
             JOIN workspace_members wm ON wm.workspace_id = i.workspace_id AND wm.user_id = r.user_id
             WHERE i.id = $1",
        )
        .bind(issue_id)
        .bind(report.owner_id)
        .execute(&mut *db)
        .await?;
        reported.push(issue_id);
    }
    Ok(reported)
}

// ---------- inbox ----------

impl FromRow<'_, PgRow> for Notification {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let actor_id: Option<Uuid> = row.try_get("actor_id")?;
        let actor_name: Option<String> = row.try_get("actor_name")?;
        let issue_id: Option<Uuid> = row.try_get("issue_id")?;
        let issue = match issue_id {
            Some(id) => {
                let team_key: String = row.try_get("team_key")?;
                let number: i32 = row.try_get("number")?;
                Some(NotificationIssue {
                    id,
                    identifier: format!("{team_key}-{number}"),
                    title: row.try_get("title")?,
                })
            }
            None => None,
        };
        let approval_id: Option<Uuid> = row.try_get("approval_id")?;
        let approval = match approval_id {
            Some(id) => Some(NotificationApproval {
                id,
                run_id: row.try_get("approval_run_id")?,
                node_id: row.try_get("approval_node_id")?,
                graph_id: row.try_get("approval_graph_id")?,
                principal: row.try_get("approval_principal")?,
                tool: row.try_get("approval_tool")?,
                reason: row.try_get("approval_reason")?,
                status: enum_col(row, "approval_status")?,
                expires_at: row.try_get("approval_expires_at")?,
            }),
            None => None,
        };
        let alert_id: Option<Uuid> = row.try_get("alert_id")?;
        let alert = match alert_id {
            Some(id) => Some(NotificationAlert {
                id,
                kind: enum_col(row, "alert_kind")?,
                severity: enum_col(row, "alert_severity")?,
                rule: row.try_get("alert_rule")?,
                title: row.try_get("alert_title")?,
                principal: row.try_get("alert_principal")?,
                run_id: row.try_get("alert_run_id")?,
                graph_id: row.try_get("alert_graph_id")?,
            }),
            None => None,
        };
        Ok(Notification {
            id: row.try_get("id")?,
            kind: enum_col(row, "kind")?,
            issue,
            approval,
            alert,
            actor: actor_id.map(|user_id| IssuePerson {
                user_id,
                name: actor_name.unwrap_or_default(),
            }),
            created_at: row.try_get("created_at")?,
            read_at: row.try_get("read_at")?,
        })
    }
}

/// Tells the issue's assignee and creator, other than the actor, that
/// something happened to it.
pub async fn notify_watchers(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    actor_id: Uuid,
    kind: NotificationKind,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO notifications (id, user_id, workspace_id, issue_id, actor_id, kind)
         SELECT gen_random_uuid(), r.user_id, i.workspace_id, i.id, $2, $3
         FROM issues i
         CROSS JOIN LATERAL (
             SELECT DISTINCT x AS user_id FROM unnest(ARRAY[i.assignee_id, i.creator_id]) x
             WHERE x IS NOT NULL AND x <> $2) r
         WHERE i.id = $1",
    )
    .bind(issue_id)
    .bind(actor_id)
    .bind(kind.as_str())
    .execute(db)
    .await?;
    Ok(())
}

/// Tells one member about an issue.
pub async fn notify(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    user_id: Uuid,
    actor_id: Uuid,
    kind: NotificationKind,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO notifications (id, user_id, workspace_id, issue_id, actor_id, kind)
         SELECT $1, $3, i.workspace_id, i.id, $4, $5 FROM issues i WHERE i.id = $2",
    )
    .bind(Uuid::now_v7())
    .bind(issue_id)
    .bind(user_id)
    .bind(actor_id)
    .bind(kind.as_str())
    .execute(db)
    .await?;
    Ok(())
}

/// Puts a reminder in the assignee's inbox for every open issue whose due
/// date has arrived and was not reminded of yet; returns how many. One
/// statement, so two servers running it at once cannot remind twice.
pub async fn remind_due(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "WITH due AS (
             UPDATE issues i SET due_notified_on = i.due_date
             FROM issue_states s
             WHERE s.id = i.state_id AND s.category NOT IN ('completed', 'canceled')
               AND i.assignee_id IS NOT NULL AND i.due_date <= current_date
               AND i.due_notified_on IS DISTINCT FROM i.due_date
             RETURNING i.id, i.workspace_id, i.assignee_id)
         INSERT INTO notifications (id, user_id, workspace_id, issue_id, actor_id, kind)
         SELECT gen_random_uuid(), assignee_id, workspace_id, id, NULL, 'due' FROM due",
    )
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}

/// A member's inbox in a workspace, newest first: only issues they can still see.
pub async fn inbox(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    limit: i64,
) -> Result<Vec<Notification>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT n.id, n.kind, n.issue_id, t.key AS team_key, i.number, i.title, n.actor_id,
                u.name AS actor_name, n.created_at, n.read_at,
                n.approval_id, a.run_id AS approval_run_id, a.node_id AS approval_node_id,
                a.graph_id AS approval_graph_id, a.principal AS approval_principal,
                a.tool AS approval_tool, a.reason AS approval_reason, a.status AS approval_status,
                a.expires_at AS approval_expires_at,
                n.alert_id, al.kind AS alert_kind, al.severity AS alert_severity, al.rule AS alert_rule,
                al.title AS alert_title, al.principal AS alert_principal, al.run_id AS alert_run_id,
                al.graph_id AS alert_graph_id
         FROM notifications n
         LEFT JOIN issues i ON i.id = n.issue_id
         LEFT JOIN teams t ON t.id = i.team_id
         LEFT JOIN approvals a ON a.id = n.approval_id
         LEFT JOIN alerts al ON al.id = n.alert_id
         JOIN workspace_members wm ON wm.workspace_id = n.workspace_id AND wm.user_id = $1
         LEFT JOIN users u ON u.id = n.actor_id
         WHERE n.user_id = $1 AND n.workspace_id = $2
           AND (n.issue_id IS NULL OR ",
        team_visible!("t", "wm", "$1"),
        ") ORDER BY n.created_at DESC, n.id DESC LIMIT $3"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Marks a member's notifications read: the given ones, or all of them.
pub async fn mark_read(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    ids: Option<&[Uuid]>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE notifications SET read_at = now()
         WHERE user_id = $1 AND workspace_id = $2 AND read_at IS NULL
           AND ($3::uuid[] IS NULL OR id = ANY($3))",
    )
    .bind(user_id)
    .bind(workspace_id)
    .bind(ids)
    .execute(db)
    .await?;
    Ok(())
}

/// Settles every inbox entry about an approval once it is decided.
pub async fn mark_read_about_approval(
    db: impl PgExecutor<'_>,
    approval_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE notifications SET read_at = now() WHERE approval_id = $1 AND read_at IS NULL",
    )
    .bind(approval_id)
    .execute(db)
    .await?;
    Ok(())
}

// ---------- projects ----------

/// A project with its team and the counts of its issues that user `$1` can
/// see, if `$1` is a member of its workspace and can see its team (a project
/// without a team is the whole workspace's).
macro_rules! project_for_user {
    ($tail:literal) => {
        concat!(
            "SELECT p.id, p.workspace_id, p.name, p.description, p.status, p.lead_id, p.target_date,
                    p.created_at, p.team_id, pt.key AS team_key, pt.name AS team_name,
                    COALESCE(c.issue_count, 0) AS issue_count,
                    COALESCE(c.closed_count, 0) AS closed_count
             FROM projects p
             JOIN workspace_members pm ON pm.workspace_id = p.workspace_id AND pm.user_id = $1
             LEFT JOIN teams pt ON pt.id = p.team_id
             LEFT JOIN LATERAL (
                SELECT count(*) AS issue_count,
                       count(*) FILTER (WHERE s.category IN ('completed', 'canceled')) AS closed_count
                FROM issues i
                JOIN teams t ON t.id = i.team_id
                JOIN issue_states s ON s.id = i.state_id
                JOIN workspace_members wm ON wm.workspace_id = i.workspace_id AND wm.user_id = $1
                WHERE i.project_id = p.id AND ",
            team_visible!("t", "wm", "$1"),
            ") c ON true WHERE (p.team_id IS NULL OR ",
            team_visible!("pt", "pm", "$1"),
            ") AND ",
            $tail
        )
    };
}

/// Projects of a workspace that `user_id` can see, oldest first: all of
/// them, or those of one team.
pub async fn projects(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    team_id: Option<Uuid>,
) -> Result<Vec<Project>, sqlx::Error> {
    sqlx::query_as(project_for_user!(
        "p.workspace_id = $2 AND ($3::uuid IS NULL OR p.team_id = $3)
         ORDER BY p.created_at, p.id"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .bind(team_id)
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

/// Fields of a project to insert.
#[derive(Debug, Clone)]
pub struct NewProject<'a> {
    pub workspace_id: Uuid,
    pub created_by: Uuid,
    pub name: &'a str,
    pub description: &'a str,
    pub status: ProjectStatus,
    pub lead_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub target_date: Option<NaiveDate>,
}

/// Inserts a project.
pub async fn create_project(
    db: impl PgExecutor<'_>,
    n: &NewProject<'_>,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO projects (id, workspace_id, name, description, status, lead_id, team_id,
                               target_date, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(n.workspace_id)
    .bind(n.name)
    .bind(n.description)
    .bind(n.status.as_str())
    .bind(n.lead_id)
    .bind(n.team_id)
    .bind(n.target_date)
    .bind(n.created_by)
    .fetch_one(db)
    .await
}

/// Writes the editable fields of a project.
pub async fn save_project(db: impl PgExecutor<'_>, p: &Project) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE projects SET name = $2, description = $3, status = $4, lead_id = $5,
                target_date = $6, team_id = $7, updated_at = now() WHERE id = $1",
    )
    .bind(p.id)
    .bind(&p.name)
    .bind(&p.description)
    .bind(p.status.as_str())
    .bind(p.lead_id)
    .bind(p.target_date)
    .bind(p.team_id)
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

/// Whether a project belongs to a workspace and `user_id` can see it (one of
/// a private team only those who see the team can).
pub async fn project_in_workspace(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(concat!(
        "SELECT EXISTS (SELECT 1 FROM projects p
             JOIN workspace_members pm ON pm.workspace_id = p.workspace_id AND pm.user_id = $1
             LEFT JOIN teams pt ON pt.id = p.team_id
             WHERE p.workspace_id = $2 AND p.id = $3 AND (p.team_id IS NULL OR ",
        team_visible!("pt", "pm", "$1"),
        "))"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .bind(id)
    .fetch_one(db)
    .await
}
