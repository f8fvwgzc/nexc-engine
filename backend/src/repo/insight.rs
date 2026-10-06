//! Reads across a whole workspace: its timeline and its relationship map.

use chrono::{DateTime, Utc};
use sqlx::{PgExecutor, Row};
use uuid::Uuid;

use crate::domain::insight::{MapEntity, MapRelation, TimelineDay, TimelineEntry, WorkspaceMap};

/// Everything that happened in workspace `$1` between `$2` (included) and
/// `$3` (excluded), from the tables that record it. Every branch is bounded
/// by the time range, so a day costs what happened that day.
macro_rules! happenings {
    () => {
        "SELECT a.created_at AS at, a.action AS kind, NULLIF(a.actor_name, '') AS actor,
                'workspace' AS entity_type, NULL::uuid AS entity_id, a.subject AS title,
                a.detail AS detail
         FROM audit_log a
         WHERE a.workspace_id = $1 AND a.created_at >= $2 AND a.created_at < $3
         UNION ALL
         SELECT i.created_at, 'issue_created', u.name, 'issue', i.id,
                t.key || '-' || i.number || ' ' || i.title, ''
         FROM issues i JOIN teams t ON t.id = i.team_id LEFT JOIN users u ON u.id = i.creator_id
         WHERE i.workspace_id = $1 AND i.created_at >= $2 AND i.created_at < $3
         UNION ALL
         SELECT e.created_at, 'issue_' || e.kind, u.name, 'issue', i.id,
                t.key || '-' || i.number || ' ' || i.title,
                CASE WHEN e.kind = 'comment' THEN left(e.body, 200)
                     ELSE COALESCE(e.from_value, 'none') || ' -> ' || COALESCE(e.to_value, 'none')
                END
         FROM issue_events e
         JOIN issues i ON i.id = e.issue_id JOIN teams t ON t.id = i.team_id
         LEFT JOIN users u ON u.id = e.actor_id
         WHERE i.workspace_id = $1 AND e.created_at >= $2 AND e.created_at < $3
         UNION ALL
         SELECT g.created_at, 'graph_created', u.name, 'graph', g.id, g.name, ''
         FROM graphs g LEFT JOIN users u ON u.id = g.owner_id
         WHERE g.workspace_id = $1 AND g.created_at >= $2 AND g.created_at < $3
         UNION ALL
         SELECT COALESCE(r.finished_at, r.created_at), 'run_' || r.status, u.name, 'graph', g.id,
                g.name, COALESCE(left(r.error, 200), '')
         FROM runs r JOIN graphs g ON g.id = r.graph_id LEFT JOIN users u ON u.id = r.owner_id
         WHERE g.workspace_id = $1 AND COALESCE(r.finished_at, r.created_at) >= $2
           AND COALESCE(r.finished_at, r.created_at) < $3
         UNION ALL
         SELECT d.created_at, 'document_added', u.name, 'document', d.id, d.name,
                CASE WHEN d.status = 'failed' THEN 'failed: ' || d.error
                     ELSE d.chunk_count || ' passages' END
         FROM documents d LEFT JOIN users u ON u.id = d.uploaded_by
         WHERE d.workspace_id = $1 AND d.created_at >= $2 AND d.created_at < $3
         UNION ALL
         SELECT max(m.created_at), 'memories_learned', NULL, 'graph', g.id, g.name,
                count(*) || ' learned'
         FROM memories m JOIN graphs g ON g.id = m.graph_id
         WHERE m.workspace_id = $1 AND m.created_at >= $2 AND m.created_at < $3
         GROUP BY g.id, g.name, (m.created_at AT TIME ZONE 'UTC')::date"
    };
}

/// What happened in a workspace in a time range, newest first.
pub async fn timeline(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<TimelineEntry>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT * FROM (",
        happenings!(),
        ") happened ORDER BY at DESC LIMIT $4"
    ))
    .bind(workspace_id)
    .bind(from)
    .bind(to)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// How much happened on each day (UTC) of a time range, latest day first;
/// days on which nothing happened are left out.
pub async fn days(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> Result<Vec<TimelineDay>, sqlx::Error> {
    sqlx::query_as(concat!(
        "SELECT (at AT TIME ZONE 'UTC')::date AS day, count(*) AS events FROM (",
        happenings!(),
        ") happened GROUP BY 1 ORDER BY 1 DESC"
    ))
    .bind(workspace_id)
    .bind(from)
    .bind(to)
    .fetch_all(db)
    .await
}

/// The kinds of things in a workspace and the ties between them, counted in
/// one statement.
pub async fn map(db: impl PgExecutor<'_>, workspace_id: Uuid) -> Result<WorkspaceMap, sqlx::Error> {
    let row = sqlx::query(
        "SELECT
           (SELECT count(*) FROM workspace_members WHERE workspace_id = $1) AS members,
           (SELECT count(*) FROM teams WHERE workspace_id = $1) AS teams,
           (SELECT count(*) FROM team_members tm JOIN teams t ON t.id = tm.team_id
             WHERE t.workspace_id = $1) AS team_memberships,
           (SELECT count(*) FROM projects WHERE workspace_id = $1) AS projects,
           (SELECT count(*) FROM issues WHERE workspace_id = $1) AS issues,
           (SELECT count(*) FROM issues WHERE workspace_id = $1 AND project_id IS NOT NULL)
             AS issues_in_projects,
           (SELECT count(*) FROM issues WHERE workspace_id = $1 AND parent_id IS NOT NULL)
             AS sub_issues,
           (SELECT count(*) FROM issues WHERE workspace_id = $1 AND assignee_id IS NOT NULL)
             AS assigned_issues,
           (SELECT count(*) FROM issues WHERE workspace_id = $1 AND graph_id IS NOT NULL)
             AS planned_issues,
           (SELECT count(*) FROM issues WHERE workspace_id = $1 AND cycle_id IS NOT NULL)
             AS issues_in_cycles,
           (SELECT count(*) FROM issue_labels il JOIN issues i ON i.id = il.issue_id
             WHERE i.workspace_id = $1) AS labelled,
           (SELECT count(*) FROM labels WHERE workspace_id = $1) AS labels,
           (SELECT count(*) FROM cycles c JOIN teams t ON t.id = c.team_id
             WHERE t.workspace_id = $1) AS cycles,
           (SELECT count(*) FROM graphs WHERE workspace_id = $1) AS graphs,
           (SELECT count(*) FROM graphs WHERE workspace_id = $1 AND team_id IS NOT NULL)
             AS team_graphs,
           (SELECT count(*) FROM runs r JOIN graphs g ON g.id = r.graph_id
             WHERE g.workspace_id = $1) AS runs,
           (SELECT count(*) FROM agents WHERE workspace_id = $1) AS agents,
           (SELECT count(*) FROM documents WHERE workspace_id = $1) AS documents,
           (SELECT count(*) FROM document_chunks WHERE workspace_id = $1) AS passages,
           (SELECT count(*) FROM memories WHERE workspace_id = $1) AS memories,
           (SELECT count(*) FROM memories WHERE workspace_id = $1 AND graph_id IS NOT NULL)
             AS graph_memories",
    )
    .bind(workspace_id)
    .fetch_one(db)
    .await?;
    let n = |column: &str| -> Result<i64, sqlx::Error> { row.try_get(column) };
    let entity = |key: &str, label: &str, count: i64| MapEntity {
        key: key.to_owned(),
        label: label.to_owned(),
        count,
    };
    let relation = |from: &str, to: &str, label: &str, count: i64| MapRelation {
        from: from.to_owned(),
        to: to.to_owned(),
        label: label.to_owned(),
        count,
    };
    Ok(WorkspaceMap {
        entities: vec![
            entity("member", "Members", n("members")?),
            entity("team", "Teams", n("teams")?),
            entity("project", "Projects", n("projects")?),
            entity("issue", "Issues", n("issues")?),
            entity("cycle", "Cycles", n("cycles")?),
            entity("label", "Labels", n("labels")?),
            entity("graph", "Graphs", n("graphs")?),
            entity("run", "Runs", n("runs")?),
            entity("agent", "Agents", n("agents")?),
            entity("document", "Documents", n("documents")?),
            entity("passage", "Passages", n("passages")?),
            entity("memory", "Memories", n("memories")?),
        ],
        relations: vec![
            relation("member", "team", "is in", n("team_memberships")?),
            relation("issue", "team", "belongs to", n("issues")?),
            relation("issue", "project", "is part of", n("issues_in_projects")?),
            relation("issue", "issue", "is a sub-issue of", n("sub_issues")?),
            relation("issue", "member", "is assigned to", n("assigned_issues")?),
            relation("issue", "cycle", "is planned in", n("issues_in_cycles")?),
            relation("issue", "label", "carries", n("labelled")?),
            relation(
                "issue",
                "graph",
                "is planned and run by",
                n("planned_issues")?,
            ),
            relation("cycle", "team", "is a time box of", n("cycles")?),
            relation("graph", "team", "belongs to", n("team_graphs")?),
            relation("run", "graph", "executes", n("runs")?),
            relation("memory", "graph", "was learned in", n("graph_memories")?),
            relation("passage", "document", "is a part of", n("passages")?),
        ],
    })
}
