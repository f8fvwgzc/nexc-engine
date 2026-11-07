//! Versions, pins and change assessments of issues and projects
//! (`crate::domain::impact`): the rows the engine reads, the versions it
//! writes back, the assessments it leaves and the inbox entries they send.

use std::str::FromStr;

use assessment_core::semver::SemVer;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::types::Json;
use sqlx::{FromRow, PgConnection, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::impact::{
    ChangeAssessment, Impact, IssueNode, NewAssessment, PlanViolation, ProjectNode,
};
use crate::domain::issue::IssuePerson;

/// How far up the parent chain, and down the sub-issues, an issue's neighbourhood reaches.
pub const ANCESTORS_MAX: i32 = 8;
pub const DESCENDANT_DEPTH: i32 = 3;
/// Most sub-issues (at any depth) and most siblings an issue's neighbourhood holds.
pub const DESCENDANTS_MAX: i64 = 300;
pub const SIBLINGS_MAX: i64 = 100;
/// Most issues of a project (its own and their sub-issues) a project's neighbourhood holds.
pub const PROJECT_ISSUES_MAX: i64 = 300;

fn version_col(row: &PgRow, col: &str) -> Result<SemVer, sqlx::Error> {
    let raw: String = row.try_get(col)?;
    SemVer::from_str(&raw).map_err(|e| sqlx::Error::ColumnDecode {
        index: col.to_owned(),
        source: Box::new(e),
    })
}

fn optional_version_col(row: &PgRow, col: &str) -> Result<Option<SemVer>, sqlx::Error> {
    match row.try_get::<Option<String>, _>(col)? {
        Some(_) => version_col(row, col).map(Some),
        None => Ok(None),
    }
}

impl FromRow<'_, PgRow> for IssueNode {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let basis_id: Option<Uuid> = row.try_get("basis_id")?;
        let basis_version = optional_version_col(row, "basis_version")?;
        Ok(IssueNode {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            team_id: row.try_get("team_id")?,
            parent_id: row.try_get("parent_id")?,
            project_id: row.try_get("project_id")?,
            cycle_id: row.try_get("cycle_id")?,
            assignee_id: row.try_get("assignee_id")?,
            creator_id: row.try_get("creator_id")?,
            title: row.try_get("title")?,
            description: row.try_get("description")?,
            state_name: row.try_get("state_name")?,
            state_category: enum_col(row, "state_category")?,
            priority: row.try_get("priority")?,
            labels: row.try_get("labels")?,
            due_date: row.try_get("due_date")?,
            updated_at: row.try_get("updated_at")?,
            version: version_col(row, "version")?,
            stored_basis: basis_id.zip(basis_version),
        })
    }
}

impl FromRow<'_, PgRow> for ProjectNode {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(ProjectNode {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            team_id: row.try_get("team_id")?,
            name: row.try_get("name")?,
            description: row.try_get("description")?,
            status: enum_col(row, "status")?,
            lead_id: row.try_get("lead_id")?,
            target_date: row.try_get("target_date")?,
            updated_at: row.try_get("updated_at")?,
            version: version_col(row, "version")?,
        })
    }
}

/// An issue with its state and label names, as [`IssueNode`] reads it.
macro_rules! node_select {
    ($tail:literal) => {
        concat!(
            "SELECT i.id, i.workspace_id, i.team_id, i.parent_id, i.project_id, i.cycle_id,
                    i.assignee_id, i.creator_id, i.title, i.description, s.name AS state_name,
                    s.category AS state_category, i.priority,
                    COALESCE((SELECT array_agg(l.name ORDER BY lower(l.name), l.id)
                              FROM issue_labels il JOIN labels l ON l.id = il.label_id
                              WHERE il.issue_id = i.id), '{}') AS labels,
                    i.due_date, i.updated_at, i.version, i.basis_id, i.basis_version
             FROM issues i JOIN issue_states s ON s.id = i.state_id
             WHERE ",
            $tail
        )
    };
}

/// One issue, locking its row until the transaction ends so that the change
/// assessed is the one written and versions move one at a time; None when
/// there is no such issue.
pub async fn issue_for_update(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Option<IssueNode>, sqlx::Error> {
    sqlx::query_as(node_select!("i.id = $1 FOR UPDATE OF i"))
        .bind(id)
        .fetch_optional(db)
        .await
}

/// The issues whose work graph `graph_id` executes, locking their rows until
/// the transaction ends in the order `repo::issues::report_run` locks them.
pub async fn issues_of_graph_for_update(
    db: &mut PgConnection,
    graph_id: Uuid,
) -> Result<Vec<IssueNode>, sqlx::Error> {
    sqlx::query_as(node_select!(
        "i.graph_id = $1 ORDER BY i.id FOR UPDATE OF i"
    ))
    .bind(graph_id)
    .fetch_all(db)
    .await
}

/// One issue as it is now (in the caller's transaction, after its write).
pub async fn issue(db: impl PgExecutor<'_>, id: Uuid) -> Result<Option<IssueNode>, sqlx::Error> {
    sqlx::query_as(node_select!("i.id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await
}

/// The issues around issue `id`, without it: its parent chain (up to
/// [`ANCESTORS_MAX`]), its sub-issues down to [`DESCENDANT_DEPTH`] (at most
/// [`DESCENDANTS_MAX`]), its siblings — the other sub-issues of its parent, or
/// the other top-level issues of its project — (at most [`SIBLINGS_MAX`], most
/// recently updated first), and the issues in `also` (where it was before a move).
pub async fn issue_neighbourhood(
    db: impl PgExecutor<'_>,
    id: Uuid,
    also: &[Uuid],
) -> Result<Vec<IssueNode>, sqlx::Error> {
    sqlx::query_as(concat!(
        "WITH RECURSIVE
             up (id, depth) AS (
                 SELECT parent_id, 1 FROM issues WHERE id = $1 AND parent_id IS NOT NULL
                 UNION ALL
                 SELECT p.parent_id, u.depth + 1 FROM issues p JOIN up u ON p.id = u.id
                 WHERE p.parent_id IS NOT NULL AND u.depth < $2),
             down (id, depth) AS (
                 SELECT c.id, 1 FROM issues c WHERE c.parent_id = $1
                 UNION ALL
                 SELECT c.id, d.depth + 1 FROM issues c JOIN down d ON c.parent_id = d.id
                 WHERE d.depth < $3),
             near AS (
                 SELECT id FROM up
                 UNION SELECT id FROM (SELECT id FROM down ORDER BY depth LIMIT $4) d
                 UNION SELECT id FROM (
                     SELECT s.id FROM issues s JOIN issues me ON me.id = $1
                     WHERE s.id <> me.id
                       AND s.workspace_id = me.workspace_id
                       AND ((me.parent_id IS NOT NULL AND s.parent_id = me.parent_id)
                            OR (me.parent_id IS NULL AND me.project_id IS NOT NULL
                                AND s.parent_id IS NULL AND s.project_id = me.project_id))
                     ORDER BY s.updated_at DESC, s.id LIMIT $5) sib
                 UNION SELECT unnest($6::uuid[]))
         ",
        node_select!("i.id IN (SELECT id FROM near) AND i.id <> $1 ORDER BY i.id")
    ))
    .bind(id)
    .bind(ANCESTORS_MAX)
    .bind(DESCENDANT_DEPTH)
    .bind(DESCENDANTS_MAX)
    .bind(SIBLINGS_MAX)
    .bind(also)
    .fetch_all(db)
    .await
}

/// A project's issues: its top-level issues and their sub-issues, most
/// recently updated first, at most [`PROJECT_ISSUES_MAX`] of each.
pub async fn project_neighbourhood(
    db: impl PgExecutor<'_>,
    project_id: Uuid,
) -> Result<Vec<IssueNode>, sqlx::Error> {
    sqlx::query_as(concat!(
        "WITH top AS (SELECT id FROM issues WHERE project_id = $1 AND parent_id IS NULL
                      ORDER BY updated_at DESC, id LIMIT $2),
              kids AS (SELECT c.id FROM issues c JOIN top t ON c.parent_id = t.id
                       ORDER BY c.updated_at DESC, c.id LIMIT $2)
         ",
        node_select!("i.id IN (SELECT id FROM top UNION SELECT id FROM kids) ORDER BY i.id")
    ))
    .bind(project_id)
    .bind(PROJECT_ISSUES_MAX)
    .fetch_all(db)
    .await
}

/// A project with its version; `lock` keeps its row until the transaction ends.
async fn project_row(
    db: impl PgExecutor<'_>,
    id: Uuid,
    lock: bool,
) -> Result<Option<ProjectNode>, sqlx::Error> {
    let query = if lock {
        "SELECT id, workspace_id, team_id, name, description, status, lead_id, target_date,
                updated_at, version
         FROM projects WHERE id = $1 FOR UPDATE"
    } else {
        "SELECT id, workspace_id, team_id, name, description, status, lead_id, target_date,
                updated_at, version
         FROM projects WHERE id = $1"
    };
    sqlx::query_as(query).bind(id).fetch_optional(db).await
}

/// One project, locking its row until the transaction ends.
pub async fn project_for_update(
    db: &mut PgConnection,
    id: Uuid,
) -> Result<Option<ProjectNode>, sqlx::Error> {
    project_row(db, id, true).await
}

/// One project as it is now.
pub async fn project(
    db: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<ProjectNode>, sqlx::Error> {
    project_row(db, id, false).await
}

/// The projects among `ids`.
pub async fn projects(
    db: impl PgExecutor<'_>,
    ids: &[Uuid],
) -> Result<Vec<ProjectNode>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_as(
        "SELECT id, workspace_id, team_id, name, description, status, lead_id, target_date,
                updated_at, version
         FROM projects WHERE id = ANY($1) ORDER BY id",
    )
    .bind(ids)
    .fetch_all(db)
    .await
}

/// The raw scores of the latest [`crate::domain::impact::HISTORY_MAX`] changes
/// that moved the version of `entity_id`, newest first: the engine's history.
pub async fn history(
    db: impl PgExecutor<'_>,
    entity_id: Uuid,
    limit: i64,
) -> Result<Vec<(DateTime<Utc>, f64)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT created_at, raw_score FROM change_assessments
         WHERE entity_id = $1 AND bump <> 'none'
         ORDER BY created_at DESC, id DESC LIMIT $2",
    )
    .bind(entity_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Writes an issue's version.
pub async fn set_issue_version(
    db: impl PgExecutor<'_>,
    id: Uuid,
    version: SemVer,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE issues SET version = $2 WHERE id = $1")
        .bind(id)
        .bind(version.to_string())
        .execute(db)
        .await?;
    Ok(())
}

/// Writes a project's version.
pub async fn set_project_version(
    db: impl PgExecutor<'_>,
    id: Uuid,
    version: SemVer,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE projects SET version = $2 WHERE id = $1")
        .bind(id)
        .bind(version.to_string())
        .execute(db)
        .await?;
    Ok(())
}

/// Pins issue `id` to the current version of what it is part of now (its
/// parent issue, else its project; nothing when neither): what it is planned
/// against from here on, after a move or once its plan was redone.
pub async fn repin(db: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE issues i
         SET basis_id = b.id, basis_version = b.version
         FROM (SELECT x.id AS issue_id,
                      COALESCE(p.id, pr.id) AS id, COALESCE(p.version, pr.version) AS version
               FROM issues x
               LEFT JOIN issues p ON p.id = x.parent_id
               LEFT JOIN projects pr ON pr.id = x.project_id AND x.parent_id IS NULL
               WHERE x.id = $1) b
         WHERE i.id = b.issue_id",
    )
    .bind(id)
    .execute(db)
    .await?;
    Ok(())
}

/// Stores an assessment; returns its id.
pub async fn insert_assessment(
    db: impl PgExecutor<'_>,
    a: &NewAssessment,
) -> Result<Uuid, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO change_assessments
             (id, workspace_id, team_id, entity_kind, entity_id, actor_id, cause, bump,
              previous_version, next_version, raw_score, score, level, recommendation, confidence,
              replan_root, summary, impacted, impacts, violations)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18,
                 $19, $20)
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(a.workspace_id)
    .bind(a.team_id)
    .bind(a.entity_kind.as_str())
    .bind(a.entity_id)
    .bind(a.actor_id)
    .bind(a.cause.as_str())
    .bind(a.bump.as_str())
    .bind(a.previous_version.map(|v| v.to_string()))
    .bind(a.next_version.map(|v| v.to_string()))
    .bind(a.raw_score)
    .bind(a.score)
    .bind(a.level.as_str())
    .bind(a.recommendation.as_str())
    .bind(a.confidence)
    .bind(a.replan_root)
    .bind(&a.summary)
    .bind(a.impacted())
    .bind(Json(&a.impacts))
    .bind(Json(&a.violations))
    .fetch_one(db)
    .await
}

impl FromRow<'_, PgRow> for ChangeAssessment {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let actor_id: Option<Uuid> = row.try_get("actor_id")?;
        let actor_name: Option<String> = row.try_get("actor_name")?;
        let Json(impacts): Json<Vec<Impact>> = row.try_get("impacts")?;
        let Json(violations): Json<Vec<PlanViolation>> = row.try_get("violations")?;
        Ok(ChangeAssessment {
            id: row.try_get("id")?,
            entity_kind: enum_col(row, "entity_kind")?,
            entity_id: row.try_get("entity_id")?,
            actor: actor_id.map(|user_id| IssuePerson {
                user_id,
                name: actor_name.unwrap_or_default(),
            }),
            cause: enum_col(row, "cause")?,
            bump: enum_col(row, "bump")?,
            previous_version: row.try_get("previous_version")?,
            next_version: row.try_get("next_version")?,
            score: row.try_get("score")?,
            level: enum_col(row, "level")?,
            recommendation: enum_col(row, "recommendation")?,
            confidence: row.try_get("confidence")?,
            replan_root: row.try_get("replan_root")?,
            summary: row.try_get("summary")?,
            impacts,
            violations,
            created_at: row.try_get("created_at")?,
        })
    }
}

/// The assessments of changes to `entity_id` or that asked something of it,
/// newest first, at most `limit`: only those of workspace `workspace_id` about
/// an item of a team `viewer` can see (a project of the whole workspace any
/// member can). What they say about other items is for the caller to redact.
pub async fn assessments(
    db: impl PgExecutor<'_>,
    viewer: Uuid,
    workspace_id: Uuid,
    entity_id: Uuid,
    limit: i64,
) -> Result<Vec<ChangeAssessment>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT a.id, a.entity_kind, a.entity_id, a.actor_id, u.name AS actor_name, a.cause,
                a.bump, a.previous_version, a.next_version, a.score, a.level, a.recommendation,
                a.confidence, a.replan_root, a.summary, a.impacts, a.violations, a.created_at
         FROM change_assessments a
         JOIN workspace_members wm ON wm.workspace_id = a.workspace_id AND wm.user_id = $1
         LEFT JOIN teams t ON t.id = a.team_id
         LEFT JOIN users u ON u.id = a.actor_id
         WHERE a.workspace_id = $2 AND (a.entity_id = $3 OR a.impacted @> ARRAY[$3::uuid])
           AND (a.team_id IS NULL OR ",
        team_visible!("t", "wm", "$1"),
        ") ORDER BY a.created_at DESC, a.id DESC LIMIT $4"
    ))
    .bind(viewer)
    .bind(workspace_id)
    .bind(entity_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// The issues and projects among `ids` in workspace `workspace_id` that
/// `viewer` can see.
pub async fn visible(
    db: impl PgExecutor<'_>,
    viewer: Uuid,
    workspace_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<Uuid>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    sqlx::query_scalar(concat!(
        "SELECT i.id FROM issues i
         JOIN teams t ON t.id = i.team_id
         JOIN workspace_members wm ON wm.workspace_id = i.workspace_id AND wm.user_id = $1
         WHERE i.workspace_id = $2 AND i.id = ANY($3) AND ",
        team_visible!("t", "wm", "$1"),
        " UNION
         SELECT p.id FROM projects p
         JOIN workspace_members pm ON pm.workspace_id = p.workspace_id AND pm.user_id = $1
         LEFT JOIN teams pt ON pt.id = p.team_id
         WHERE p.workspace_id = $2 AND p.id = ANY($3) AND (p.team_id IS NULL OR ",
        team_visible!("pt", "pm", "$1"),
        ")"
    ))
    .bind(viewer)
    .bind(workspace_id)
    .bind(ids)
    .fetch_all(db)
    .await
}

/// Tells the owners of the issues `issue_ids` — their assignee and creator —
/// that the actor's change affects them (`impact`), and the leads of the
/// projects `project_ids` that it affects their project, as an entry about
/// issue `about` (the changed issue; none for a project's own change). Never
/// the actor (`None` for a change nobody made), only members who can see the
/// issue the entry is about.
pub async fn notify_impacted(
    db: impl PgExecutor<'_>,
    actor_id: Option<Uuid>,
    issue_ids: &[Uuid],
    project_ids: &[Uuid],
    about: Option<Uuid>,
) -> Result<u64, sqlx::Error> {
    if issue_ids.is_empty() && (project_ids.is_empty() || about.is_none()) {
        return Ok(0);
    }
    let done = sqlx::query(concat!(
        "INSERT INTO notifications (id, user_id, workspace_id, issue_id, actor_id, kind)
         SELECT gen_random_uuid(), r.user_id, i.workspace_id, i.id, $2::uuid, 'impact'
         FROM (SELECT i.id, x AS user_id FROM issues i
                   CROSS JOIN LATERAL unnest(ARRAY[i.assignee_id, i.creator_id]) x
               WHERE i.id = ANY($1)
               UNION
               SELECT $4::uuid, p.lead_id FROM projects p
               WHERE p.id = ANY($3) AND $4::uuid IS NOT NULL) r
         JOIN issues i ON i.id = r.id
         JOIN teams t ON t.id = i.team_id
         JOIN workspace_members wm ON wm.workspace_id = i.workspace_id AND wm.user_id = r.user_id
         WHERE r.user_id IS NOT NULL AND r.user_id IS DISTINCT FROM $2::uuid AND ",
        team_visible!("t", "wm", "wm.user_id")
    ))
    .bind(issue_ids)
    .bind(actor_id)
    .bind(project_ids)
    .bind(about)
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}
