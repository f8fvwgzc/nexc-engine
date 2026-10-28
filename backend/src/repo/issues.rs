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
    Issue, IssueChange, IssueEvent, IssueEventKind, IssuePerson, IssueRef, IssueState,
    IssueSummary, Label, NodeOutcome, Notification, NotificationAlert, NotificationApproval,
    NotificationIssue, NotificationKind, Project, ProjectStatus, RUN_REPORT_MAX_CHARS, Reaction,
    STARTER_STATES, StateCategory, SubIssueCount, TeamRef,
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

impl FromRow<'_, PgRow> for IssueSummary {
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
        // `[total, closed]`, from one look at the sub-issues.
        let sub: Vec<i64> = row.try_get("sub_counts")?;
        let run_status: Option<String> = row.try_get("run_status")?;
        Ok(IssueSummary {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            team_id,
            identifier: format!("{team_key}-{number}"),
            number,
            title: row.try_get("title")?,
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
                total: sub.first().copied().unwrap_or_default(),
                closed: sub.get(1).copied().unwrap_or_default(),
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
            // A status this version does not know reads as none rather than failing the list.
            run_status: run_status.and_then(|s| s.parse().ok()),
        })
    }
}

impl FromRow<'_, PgRow> for Issue {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(IssueSummary::from_row(row)?.with_description(row.try_get("description")?))
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
/// a member of the issue's workspace and able to see its team; with its
/// description (`issue_for_user!`) or without (`issue_summary_for_user!`).
///
/// What each row adds is read by subqueries in the select list, which
/// PostgreSQL runs only for the rows a sort and `LIMIT` keep: the labels, the
/// sub-issue counts (one look through `issues_parent_state_idx`) and the
/// status of the latest run of the issue's graph (`runs_graph_idx`).
macro_rules! issue_select {
    ($description:literal, $tail:literal) => {
        concat!(
            "SELECT i.id, i.workspace_id, i.team_id, t.key AS team_key, i.number, i.title, ",
            $description,
            " i.priority, i.assignee_id, au.name AS assignee_name, i.agent_id,
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
                    (SELECT ARRAY[count(*),
                                  count(*) FILTER (WHERE cs.category IN ('completed', 'canceled'))]
                     FROM issues c JOIN issue_states cs ON cs.id = c.state_id
                     WHERE c.parent_id = i.id) AS sub_counts,
                    (SELECT r.status FROM runs r WHERE r.graph_id = i.graph_id
                     ORDER BY r.created_at DESC LIMIT 1) AS run_status
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

macro_rules! issue_for_user {
    ($tail:literal) => {
        issue_select!("i.description,", $tail)
    };
}

macro_rules! issue_summary_for_user {
    ($tail:literal) => {
        issue_select!("", $tail)
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

/// Issues of a workspace that `user_id` can see, most recently updated
/// first, without their descriptions.
///
/// Only the filters that are given are in the statement, each combination a
/// statement of its own. One statement holding every filter as `($n IS NULL
/// OR col = $n)` would be planned once for all of them after a few runs, and
/// that plan walks the whole workspace for a sub-issue or project list
/// (26 ms instead of 1 on 6,000 issues), where its own index answers at once.
pub async fn list(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    f: &IssueFilter,
) -> Result<Vec<IssueSummary>, sqlx::Error> {
    let mut sql = String::from(issue_summary_for_user!("i.workspace_id = $2"));
    // A `?` in a clause is its parameter: the next one, in the order they are bound below.
    let mut args = 2;
    let mut push = |clause: &str| {
        if clause.contains('?') {
            args += 1;
            sql.push_str(&clause.replace('?', &format!("${args}")));
        } else {
            sql.push_str(clause);
        }
    };
    let uuids = [
        (f.team_id, " AND i.team_id = ?"),
        (f.assignee_id, " AND i.assignee_id = ?"),
        (f.project_id, " AND i.project_id = ?"),
        (f.parent_id, " AND i.parent_id = ?"),
        (f.cycle_id, " AND i.cycle_id = ?"),
        (f.creator_id, " AND i.creator_id = ?"),
        (
            f.label_id,
            " AND EXISTS (SELECT 1 FROM issue_labels fl WHERE fl.issue_id = i.id AND fl.label_id = ?)",
        ),
    ];
    for (_, clause) in uuids.iter().filter(|(id, _)| id.is_some()) {
        push(clause);
    }
    if f.q.is_some() {
        push(" AND (i.title ILIKE '%' || ? || '%' OR (t.key || '-' || i.number) ILIKE ? || '%')");
    }
    if f.open_only {
        push(" AND s.category NOT IN ('completed', 'canceled')");
    }
    push(" ORDER BY i.updated_at DESC, i.id LIMIT ?");
    let mut query = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(user_id)
        .bind(workspace_id);
    for id in uuids.iter().filter_map(|(id, _)| *id) {
        query = query.bind(id);
    }
    if let Some(q) = &f.q {
        query = query.bind(q.clone());
    }
    query.bind(f.limit).fetch_all(db).await
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

/// Whether `user_id` can see issue `id` of workspace `workspace_id` (an
/// index lookup, without the issue's columns).
pub async fn visible_in(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(concat!(
        "SELECT EXISTS (SELECT 1 FROM issues i
             JOIN teams t ON t.id = i.team_id
             JOIN workspace_members wm ON wm.workspace_id = i.workspace_id AND wm.user_id = $1
             WHERE i.id = $3 AND i.workspace_id = $2 AND ",
        team_visible!("t", "wm", "$1"),
        ")"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .bind(id)
    .fetch_one(db)
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
    /// The issue it is part of, checked by the caller.
    pub parent_id: Option<Uuid>,
    /// A cycle of its team, checked by the caller.
    pub cycle_id: Option<Uuid>,
    pub due_date: Option<NaiveDate>,
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
                             assignee_id, agent_id, project_id, creator_id, completed_at, github_url,
                             parent_id, cycle_id, due_date)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                 CASE WHEN $13 THEN now() END, $14, $15, $16, $17)
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
    .bind(n.parent_id)
    .bind(n.cycle_id)
    .bind(n.due_date)
    .fetch_one(&mut *db)
    .await
}

/// What a change to an issue moves besides its own fields; `None` leaves a
/// dimension as it is, `Some(None)` clears it.
#[derive(Debug, Clone, Copy, Default)]
pub struct IssueMoves {
    /// The issue it is part of.
    pub parent_id: Option<Option<Uuid>>,
    /// The cycle of its team it is planned in.
    pub cycle_id: Option<Option<Uuid>>,
    /// The day it is due.
    pub due_date: Option<Option<NaiveDate>>,
}

/// Writes the editable fields of an issue and `moves`, in one statement.
/// `completed_at` follows the state: set when the issue enters a closed
/// state, cleared when it is reopened.
pub async fn save(
    db: impl PgExecutor<'_>,
    issue: &Issue,
    moves: &IssueMoves,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE issues SET title = $2, description = $3, state_id = $4, priority = $5,
                assignee_id = $6, agent_id = $7, project_id = $8, github_url = $10,
                parent_id = CASE WHEN $11 THEN $12 ELSE parent_id END,
                cycle_id = CASE WHEN $13 THEN $14 ELSE cycle_id END,
                due_date = CASE WHEN $15 THEN $16 ELSE due_date END,
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
    .bind(moves.parent_id.is_some())
    .bind(moves.parent_id.flatten())
    .bind(moves.cycle_id.is_some())
    .bind(moves.cycle_id.flatten())
    .bind(moves.due_date.is_some())
    .bind(moves.due_date.flatten())
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

/// Makes `ids` the labels of an issue, in one statement.
pub async fn set_labels(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "WITH gone AS (
             DELETE FROM issue_labels WHERE issue_id = $1 AND label_id <> ALL($2)),
         added AS (
             INSERT INTO issue_labels (issue_id, label_id) SELECT $1, unnest($2::uuid[])
             ON CONFLICT DO NOTHING)
         UPDATE issues SET updated_at = now() WHERE id = $1",
    )
    .bind(issue_id)
    .bind(ids)
    .execute(db)
    .await?;
    Ok(())
}

// ---------- timeline ----------

impl FromRow<'_, PgRow> for IssueEvent {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let actor_id: Option<Uuid> = row.try_get("actor_id")?;
        let actor_name: Option<String> = row.try_get("actor_name")?;
        let Json(reactions): Json<Vec<Reaction>> = row.try_get("reactions")?;
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
            parent_id: row.try_get("parent_id")?,
            reactions,
        })
    }
}

/// Timeline entries with their actor and their reactions as user `$1` reads
/// them: each emoji once, in the order it was first given, with its count,
/// whether `$1` gave it and the latest ten who did
/// ([`crate::domain::issue::REACTION_USERS_MAX`]). The reactions are read in
/// the same statement, one probe of their primary key per entry.
macro_rules! event_select {
    ($tail:literal) => {
        concat!(
            "SELECT e.id, e.issue_id, e.kind, e.actor_id, u.name AS actor_name, e.body,
                    e.from_value, e.to_value, e.created_at, e.edited_at, e.run_id, e.parent_id,
                    COALESCE(r.reactions, '[]'::jsonb) AS reactions
             FROM issue_events e
             LEFT JOIN users u ON u.id = e.actor_id
             LEFT JOIN LATERAL (
                 SELECT jsonb_agg(jsonb_build_object('emoji', g.emoji, 'count', g.count,
                                                     'reacted', g.reacted, 'users', g.users)
                                  ORDER BY g.first_at, g.emoji) AS reactions
                 FROM (SELECT x.emoji, count(*) AS count, bool_or(x.user_id = $1) AS reacted,
                              min(x.created_at) AS first_at,
                              to_jsonb((array_agg(jsonb_build_object('user_id', x.user_id,
                                                                     'name', xu.name)
                                                  ORDER BY x.created_at DESC, x.user_id))[1:10])
                                  AS users
                       FROM issue_event_reactions x JOIN users xu ON xu.id = x.user_id
                       WHERE x.event_id = e.id
                       GROUP BY x.emoji) g) r ON true
             WHERE ",
            $tail
        )
    };
}

/// The timeline of an issue, oldest first, as `viewer` reads it.
pub async fn events(
    db: impl PgExecutor<'_>,
    viewer: Uuid,
    issue_id: Uuid,
) -> Result<Vec<IssueEvent>, sqlx::Error> {
    sqlx::query_as(event_select!("e.issue_id = $2 ORDER BY e.created_at, e.id"))
        .bind(viewer)
        .bind(issue_id)
        .fetch_all(db)
        .await
}

/// One entry of an issue's timeline, of any kind, as `viewer` reads it.
pub async fn find_event(
    db: impl PgExecutor<'_>,
    viewer: Uuid,
    issue_id: Uuid,
    id: Uuid,
) -> Result<Option<IssueEvent>, sqlx::Error> {
    sqlx::query_as(event_select!("e.issue_id = $2 AND e.id = $3"))
        .bind(viewer)
        .bind(issue_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// One comment of an issue, as `viewer` reads it.
pub async fn find_comment(
    db: impl PgExecutor<'_>,
    viewer: Uuid,
    issue_id: Uuid,
    id: Uuid,
) -> Result<Option<IssueEvent>, sqlx::Error> {
    sqlx::query_as(event_select!(
        "e.issue_id = $2 AND e.id = $3 AND e.kind = 'comment'"
    ))
    .bind(viewer)
    .bind(issue_id)
    .bind(id)
    .fetch_optional(db)
    .await
}

/// What a reply or a reaction needs to know of the entry it is about.
#[derive(Debug, Clone, Copy)]
pub struct EventRef {
    pub kind: IssueEventKind,
    /// Who wrote it; `None` for a run report and once the account is gone.
    pub actor_id: Option<Uuid>,
    /// The comment it replies to.
    pub parent_id: Option<Uuid>,
}

impl FromRow<'_, PgRow> for EventRef {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(EventRef {
            kind: enum_col(row, "kind")?,
            actor_id: row.try_get("actor_id")?,
            parent_id: row.try_get("parent_id")?,
        })
    }
}

/// One entry of the issue's timeline, kept from being deleted until the
/// transaction ends (`FOR KEY SHARE`, which edits do not wait for), so the
/// reply or the reaction written in it has the entry to point at; None when
/// the issue has no such entry.
pub async fn event_for_share(
    db: &mut PgConnection,
    issue_id: Uuid,
    id: Uuid,
) -> Result<Option<EventRef>, sqlx::Error> {
    sqlx::query_as(
        "SELECT kind, actor_id, parent_id FROM issue_events
         WHERE issue_id = $1 AND id = $2 FOR KEY SHARE",
    )
    .bind(issue_id)
    .bind(id)
    .fetch_optional(db)
    .await
}

/// Adds a comment, or a reply to the comment `parent_id`, and returns its id.
pub async fn add_comment(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    actor_id: Uuid,
    body: &str,
    parent_id: Option<Uuid>,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO issue_events (id, issue_id, actor_id, kind, body, parent_id)
         VALUES ($1, $2, $3, 'comment', $4, $5) RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(issue_id)
    .bind(actor_id)
    .bind(body)
    .bind(parent_id)
    .fetch_one(db)
    .await
}

/// Gives an entry the person's emoji; returns whether it is new (giving the
/// same emoji again changes nothing).
pub async fn add_reaction(
    db: impl PgExecutor<'_>,
    event_id: Uuid,
    user_id: Uuid,
    emoji: &str,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "INSERT INTO issue_event_reactions (event_id, user_id, emoji) VALUES ($1, $2, $3)
         ON CONFLICT DO NOTHING",
    )
    .bind(event_id)
    .bind(user_id)
    .bind(emoji)
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Takes the person's emoji back from an entry, if they gave it.
pub async fn remove_reaction(
    db: impl PgExecutor<'_>,
    event_id: Uuid,
    user_id: Uuid,
    emoji: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "DELETE FROM issue_event_reactions WHERE event_id = $1 AND user_id = $2 AND emoji = $3",
    )
    .bind(event_id)
    .bind(user_id)
    .bind(emoji)
    .execute(db)
    .await?;
    Ok(())
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

// ---------- mentions ----------

/// An agent a comment mentions, as much as handing it the issue needs.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct MentionedAgent {
    pub id: Uuid,
    pub name: String,
    pub role: String,
}

/// The agents among `ids` that belong to the workspace and take work
/// (`active`), in the order of `ids`.
pub async fn active_agents(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<MentionedAgent>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as(
        "SELECT a.id, a.name, a.role
         FROM unnest($2::uuid[]) WITH ORDINALITY m (id, n)
         JOIN agents a ON a.id = m.id
         WHERE a.workspace_id = $1 AND a.status = 'active'
         ORDER BY m.n",
    )
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(db)
    .await
}

/// Delegates the issue to the agent unless it already is to one; returns
/// whether it was.
pub async fn delegate(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    agent_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE issues SET agent_id = $2, updated_at = now() WHERE id = $1 AND agent_id IS NULL",
    )
    .bind(issue_id)
    .bind(agent_id)
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Records in the issue's timeline what the actor's mention asked of an
/// agent: an `agent` entry with the agent's name and `what`
/// ([`crate::domain::issue::AGENT_DELEGATED`],
/// [`crate::domain::issue::AGENT_ASKED_TO_PLAN`]).
pub async fn record_agent(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    actor_id: Uuid,
    agent_name: &str,
    what: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO issue_events (id, issue_id, actor_id, kind, body, to_value)
         VALUES ($1, $2, $3, 'agent', $4, $5)",
    )
    .bind(Uuid::now_v7())
    .bind(issue_id)
    .bind(actor_id)
    .bind(what)
    .bind(agent_name)
    .execute(db)
    .await?;
    Ok(())
}

/// Whether the graph has work under way: a run queued or running, or a plan
/// still streaming. A mention does not ask its agents for another then.
pub async fn graph_busy(db: impl PgExecutor<'_>, graph_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM runs WHERE graph_id = $1 AND status IN ('queued', 'running'))
             OR EXISTS (SELECT 1 FROM plans WHERE graph_id = $1 AND status = 'streaming')",
    )
    .bind(graph_id)
    .fetch_one(db)
    .await
}

/// The issue's graph, if it has one, locking the issue's row until the
/// transaction ends; None when there is no such issue.
pub async fn graph_id_for_update(
    db: &mut PgConnection,
    issue_id: Uuid,
) -> Result<Option<Option<Uuid>>, sqlx::Error> {
    sqlx::query_scalar("SELECT graph_id FROM issues WHERE id = $1 FOR UPDATE")
        .bind(issue_id)
        .fetch_optional(db)
        .await
}

/// A comment's text, locking it until the transaction ends, so edits made at
/// the same moment take turns; None when there is no such comment.
pub async fn comment_body_for_update(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT body FROM issue_events WHERE id = $1 AND kind = 'comment' FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(db)
    .await
}

/// Records changes to an issue in its timeline, in one statement.
pub async fn record_changes(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    actor_id: Uuid,
    changes: &[IssueChange],
) -> Result<(), sqlx::Error> {
    if changes.is_empty() {
        return Ok(());
    }
    // Ids are made one after another, so the entries keep the order of the changes.
    let ids: Vec<Uuid> = changes.iter().map(|_| Uuid::now_v7()).collect();
    let kinds: Vec<&str> = changes.iter().map(|c| c.kind.as_str()).collect();
    let from: Vec<Option<&str>> = changes.iter().map(|c| c.from.as_deref()).collect();
    let to: Vec<Option<&str>> = changes.iter().map(|c| c.to.as_deref()).collect();
    sqlx::query(
        "INSERT INTO issue_events (id, issue_id, actor_id, kind, from_value, to_value)
         SELECT c.id, $1, $2, c.kind, c.from_value, c.to_value
         FROM unnest($3::uuid[], $4::text[], $5::text[], $6::text[])
              AS c (id, kind, from_value, to_value)",
    )
    .bind(issue_id)
    .bind(actor_id)
    .bind(&ids)
    .bind(&kinds)
    .bind(&from)
    .bind(&to)
    .execute(db)
    .await?;
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
            emoji: row.try_get("emoji")?,
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
    notify_watchers_except(db, issue_id, actor_id, kind, &[]).await
}

/// [`notify_watchers`], leaving out `except`: the members a comment's
/// mentions already told, who get one entry for it, not two.
pub async fn notify_watchers_except(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    actor_id: Uuid,
    kind: NotificationKind,
    except: &[Uuid],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO notifications (id, user_id, workspace_id, issue_id, actor_id, kind)
         SELECT gen_random_uuid(), r.user_id, i.workspace_id, i.id, $2, $3
         FROM issues i
         CROSS JOIN LATERAL (
             SELECT DISTINCT x AS user_id FROM unnest(ARRAY[i.assignee_id, i.creator_id]) x
             WHERE x IS NOT NULL AND x <> $2 AND NOT (x = ANY($4::uuid[]))) r
         WHERE i.id = $1",
    )
    .bind(issue_id)
    .bind(actor_id)
    .bind(kind.as_str())
    .bind(except)
    .execute(db)
    .await?;
    Ok(())
}

/// Tells the members `user_ids` that the actor mentioned them on the issue,
/// and returns those told ([`notify_members`]).
pub async fn notify_mentioned(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    actor_id: Uuid,
    user_ids: &[Uuid],
) -> Result<Vec<Uuid>, sqlx::Error> {
    notify_members(db, issue_id, actor_id, user_ids, NotificationKind::Mention).await
}

/// Tells the members `user_ids` what the actor did on the issue (`kind`),
/// and returns those told. One statement: only members of the issue's
/// workspace who can see its team are, never the actor; other ids are
/// dropped without a word, so a mention or a reply reveals nothing about who
/// can see what.
pub async fn notify_members(
    db: impl PgExecutor<'_>,
    issue_id: Uuid,
    actor_id: Uuid,
    user_ids: &[Uuid],
    kind: NotificationKind,
) -> Result<Vec<Uuid>, sqlx::Error> {
    if user_ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_scalar(concat!(
        "INSERT INTO notifications (id, user_id, workspace_id, issue_id, actor_id, kind)
         SELECT gen_random_uuid(), wm.user_id, i.workspace_id, i.id, $2, $4
         FROM issues i
         JOIN teams t ON t.id = i.team_id
         JOIN workspace_members wm ON wm.workspace_id = i.workspace_id
         WHERE i.id = $1 AND wm.user_id = ANY($3) AND wm.user_id <> $2 AND ",
        team_visible!("t", "wm", "wm.user_id"),
        " RETURNING user_id"
    ))
    .bind(issue_id)
    .bind(actor_id)
    .bind(user_ids)
    .bind(kind.as_str())
    .fetch_all(db)
    .await
}

/// Tells the author of the comment `event_id` that the actor reacted to it
/// with `emoji`, as [`notify_members`] would: never an author reacting to
/// their own comment, nor one who can no longer see the issue. One entry per
/// actor, emoji and comment a day: an emoji taken back and given again tells
/// nobody twice, the same one given to the author's next comment does.
pub async fn notify_reaction(
    db: impl PgExecutor<'_>,
    event_id: Uuid,
    actor_id: Uuid,
    emoji: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(concat!(
        "INSERT INTO notifications (id, user_id, workspace_id, issue_id, actor_id, kind, emoji,
                                    event_id)
         SELECT $1, e.actor_id, i.workspace_id, i.id, $3, 'reaction', $4, e.id
         FROM issue_events e
         JOIN issues i ON i.id = e.issue_id
         JOIN teams t ON t.id = i.team_id
         JOIN workspace_members wm ON wm.workspace_id = i.workspace_id AND wm.user_id = e.actor_id
         WHERE e.id = $2 AND e.kind = 'comment' AND e.actor_id <> $3 AND ",
        team_visible!("t", "wm", "e.actor_id"),
        " AND NOT EXISTS (
               SELECT 1 FROM notifications n
               WHERE n.event_id = e.id AND n.user_id = e.actor_id AND n.actor_id = $3
                 AND n.kind = 'reaction' AND n.emoji = $4
                 AND n.created_at > now() - interval '1 day')"
    ))
    .bind(Uuid::now_v7())
    .bind(event_id)
    .bind(actor_id)
    .bind(emoji)
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
                u.name AS actor_name, n.emoji, n.created_at, n.read_at,
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

/// How many of the newest `window` notifications a member's inbox shows in
/// a workspace ([`inbox`]: the same rows, on issues they can still see) are
/// unread. Counting further back would count what the inbox can neither show
/// nor mark read. Reads `notifications_user_idx`, newest first.
pub async fn unread_count(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    window: i64,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(concat!(
        "SELECT count(*) FILTER (WHERE read_at IS NULL) FROM (
             SELECT n.read_at FROM notifications n
             JOIN workspace_members wm ON wm.workspace_id = n.workspace_id AND wm.user_id = $1
             LEFT JOIN issues i ON i.id = n.issue_id
             LEFT JOIN teams t ON t.id = i.team_id
             WHERE n.user_id = $1 AND n.workspace_id = $2
               AND (n.issue_id IS NULL OR ",
        team_visible!("t", "wm", "$1"),
        ") ORDER BY n.created_at DESC, n.id DESC LIMIT $3) shown"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .bind(window)
    .fetch_one(db)
    .await
}

// ---------- projects ----------

/// A project with its team and the counts of its issues that user `$1` can
/// see, if `$1` is a member of its workspace and can see its team (a project
/// without a team is the whole workspace's).
///
/// The counts read only `issues_project_state_idx (project_id, state_id,
/// team_id)`, never the issues themselves: the issues of a project are in its
/// workspace, so the caller's membership there (`pm`) decides which of their
/// teams they see.
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
                JOIN teams t ON t.id = i.team_id AND t.workspace_id = p.workspace_id
                JOIN issue_states s ON s.id = i.state_id
                WHERE i.project_id = p.id AND ",
            team_visible!("t", "pm", "$1"),
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

/// Projects of a workspace that `user_id` can see whose name contains `q`
/// (any case), oldest first, at most `limit`: `(id, name, status)`, for search.
pub async fn search_projects(
    db: impl PgExecutor<'_>,
    user_id: Uuid,
    workspace_id: Uuid,
    q: &str,
    limit: i64,
) -> Result<Vec<(Uuid, String, String)>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT p.id, p.name, p.status FROM projects p
         JOIN workspace_members pm ON pm.workspace_id = p.workspace_id AND pm.user_id = $1
         LEFT JOIN teams pt ON pt.id = p.team_id
         WHERE p.workspace_id = $2 AND strpos(lower(p.name), lower($3)) > 0
           AND (p.team_id IS NULL OR ",
        team_visible!("pt", "pm", "$1"),
        ") ORDER BY p.created_at, p.id LIMIT $4"
    ))
    .bind(user_id)
    .bind(workspace_id)
    .bind(q)
    .bind(limit)
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
