//! Re-plan proposals (`crate::engine::replan`): taking up the re-plan an
//! assessment asks for, at most one open proposal per root, and the claims
//! that start the planner once the debounce is over.

use std::time::Duration;

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgConnection, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::impact::ImpactEntityKind;

/// A re-plan an assessment asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProposal {
    pub workspace_id: Uuid,
    pub root_kind: ImpactEntityKind,
    pub root_id: Uuid,
    /// The root's team; `None` for a project of the whole workspace.
    pub team_id: Option<Uuid>,
    pub assessment_id: Uuid,
    /// Who made the change; `None` for one nobody made (a run moving its issue).
    pub owner_id: Option<Uuid>,
}

/// What taking up a re-plan did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Proposed {
    /// A proposal still waiting out its debounce now answers this assessment, and waits longer.
    Extended,
    /// A new proposal; `superseded` open ones of the same root were replaced.
    New { superseded: u64 },
}

/// The kind and team of the issue or project `id`; `None` when there is no such item.
pub async fn root(
    db: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<(ImpactEntityKind, Option<Uuid>)>, sqlx::Error> {
    let row: Option<(String, Option<Uuid>)> = sqlx::query_as(
        "SELECT 'issue', team_id FROM issues WHERE id = $1
         UNION ALL SELECT 'project', team_id FROM projects WHERE id = $1
         LIMIT 1",
    )
    .bind(id)
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(kind, team)| {
        let kind = if kind == "project" {
            ImpactEntityKind::Project
        } else {
            ImpactEntityKind::Issue
        };
        (kind, team)
    }))
}

/// Takes up the re-plan of `p.root_id` that assessment `p.assessment_id`
/// asks for, in the caller's transaction: a proposal of that root still
/// waiting out its debounce now answers this assessment and waits `debounce`
/// longer (at most ten debounce periods after it was made); otherwise the
/// open one (being started, or started) is superseded and a new one waits
/// `debounce`. A change nobody made only extends a waiting proposal: it never
/// replaces a plan someone's change asked for. Returns the proposal's id,
/// `None` when nothing was taken up.
pub async fn propose(
    db: &mut PgConnection,
    p: &NewProposal,
    debounce: Duration,
) -> Result<Option<(Uuid, Proposed)>, sqlx::Error> {
    let secs = debounce.as_secs_f64();
    let extended: Option<Uuid> = sqlx::query_scalar(
        "UPDATE replan_proposals
         SET assessment_id = $2, owner_id = COALESCE($3, owner_id),
             not_before = LEAST(now() + make_interval(secs => $4),
                                created_at + make_interval(secs => $4 * 10)),
             updated_at = now()
         WHERE root_id = $1 AND status = 'pending'
         RETURNING id",
    )
    .bind(p.root_id)
    .bind(p.assessment_id)
    .bind(p.owner_id)
    .bind(secs)
    .fetch_optional(&mut *db)
    .await?;
    if let Some(id) = extended {
        return Ok(Some((id, Proposed::Extended)));
    }
    if p.owner_id.is_none() {
        return Ok(None);
    }
    let superseded = sqlx::query(
        "UPDATE replan_proposals SET status = 'superseded', updated_at = now()
         WHERE root_id = $1 AND status IN ('starting', 'started')",
    )
    .bind(p.root_id)
    .execute(&mut *db)
    .await?
    .rows_affected();
    // Another change to the same root may have made one meanwhile: that one waits longer instead.
    let id: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO replan_proposals
             (id, workspace_id, team_id, root_kind, root_id, assessment_id, owner_id, status,
              not_before)
         VALUES ($1, $2, $3, $4, $5, $6, $7, 'pending', now() + make_interval(secs => $8))
         ON CONFLICT (root_id) WHERE status IN ('pending', 'starting', 'started')
         DO UPDATE SET assessment_id = EXCLUDED.assessment_id,
                       owner_id = COALESCE(EXCLUDED.owner_id, replan_proposals.owner_id),
                       not_before = LEAST(EXCLUDED.not_before,
                                          replan_proposals.created_at
                                              + make_interval(secs => $8 * 10)),
                       updated_at = now()
             WHERE replan_proposals.status = 'pending'
         RETURNING id",
    )
    .bind(Uuid::now_v7())
    .bind(p.workspace_id)
    .bind(p.team_id)
    .bind(p.root_kind.as_str())
    .bind(p.root_id)
    .bind(p.assessment_id)
    .bind(p.owner_id)
    .bind(secs)
    .fetch_optional(&mut *db)
    .await?;
    match id {
        Some(id) => Ok(Some((id, Proposed::New { superseded }))),
        // Started by another backend in between: that one stands.
        None => Ok(sqlx::query_scalar(
            "SELECT id FROM replan_proposals
             WHERE root_id = $1 AND status IN ('pending', 'starting', 'started')",
        )
        .bind(p.root_id)
        .fetch_optional(&mut *db)
        .await?
        .map(|id| (id, Proposed::Extended))),
    }
}

/// Points assessment `assessment_id` at the proposal that took up its re-plan.
pub async fn link_assessment(
    db: impl PgExecutor<'_>,
    assessment_id: Uuid,
    replan_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE change_assessments SET replan_id = $2 WHERE id = $1")
        .bind(assessment_id)
        .bind(replan_id)
        .execute(db)
        .await?;
    Ok(())
}

/// A proposal claimed to start the planner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claimed {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub team_id: Option<Uuid>,
    pub root_kind: ImpactEntityKind,
    pub root_id: Uuid,
    pub assessment_id: Uuid,
    pub owner_id: Option<Uuid>,
}

impl FromRow<'_, PgRow> for Claimed {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        Ok(Claimed {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            team_id: row.try_get("team_id")?,
            root_kind: enum_col(row, "root_kind")?,
            root_id: row.try_get("root_id")?,
            assessment_id: row.try_get("assessment_id")?,
            owner_id: row.try_get("owner_id")?,
        })
    }
}

/// Claims up to `limit` proposals whose debounce is over, oldest due first,
/// for this backend to start (another backend skips them), after putting
/// back the ones a backend claimed and never finished (it stopped).
pub async fn claim_due(db: &mut PgConnection, limit: i64) -> Result<Vec<Claimed>, sqlx::Error> {
    sqlx::query(
        "UPDATE replan_proposals SET status = 'pending', updated_at = now()
         WHERE status = 'starting' AND updated_at < now() - interval '5 minutes'",
    )
    .execute(&mut *db)
    .await?;
    sqlx::query_as(
        "UPDATE replan_proposals SET status = 'starting', updated_at = now()
         WHERE id IN (SELECT id FROM replan_proposals
                      WHERE status = 'pending' AND not_before <= now()
                      ORDER BY not_before LIMIT $1 FOR UPDATE SKIP LOCKED)
         RETURNING id, workspace_id, team_id, root_kind, root_id, assessment_id, owner_id",
    )
    .bind(limit)
    .fetch_all(&mut *db)
    .await
}

/// How long until the next waiting proposal is due (zero when one is); `None` when none waits.
pub async fn next_due(db: impl PgExecutor<'_>) -> Result<Option<Duration>, sqlx::Error> {
    let secs: Option<f64> = sqlx::query_scalar(
        "SELECT GREATEST(EXTRACT(EPOCH FROM min(not_before) - now()), 0)::float8
         FROM replan_proposals WHERE status = 'pending'",
    )
    .fetch_one(db)
    .await?;
    Ok(secs.map(|s| Duration::from_secs_f64(s.max(0.0))))
}

/// Records the plan drafted for a claimed proposal: started (unless a newer
/// assessment superseded it meanwhile; the plan is linked all the same).
pub async fn started(
    db: impl PgExecutor<'_>,
    id: Uuid,
    graph_id: Uuid,
    plan_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE replan_proposals
         SET graph_id = $2, plan_id = $3, updated_at = now(),
             status = CASE WHEN status = 'starting' THEN 'started' ELSE status END
         WHERE id = $1",
    )
    .bind(id)
    .bind(graph_id)
    .bind(plan_id)
    .execute(db)
    .await?;
    Ok(())
}

/// Records why a claimed proposal drafts no plan.
pub async fn skipped(
    db: impl PgExecutor<'_>,
    id: Uuid,
    graph_id: Option<Uuid>,
    error: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE replan_proposals
         SET error = $3, graph_id = COALESCE($2, graph_id), updated_at = now(),
             status = CASE WHEN status = 'starting' THEN 'skipped' ELSE status END
         WHERE id = $1",
    )
    .bind(id)
    .bind(graph_id)
    .bind(error)
    .execute(db)
    .await?;
    Ok(())
}

/// Puts a claimed proposal back to wait `wait` more: its graph is busy drafting another plan, or
/// something failed that may pass.
pub async fn postpone(
    db: impl PgExecutor<'_>,
    id: Uuid,
    graph_id: Option<Uuid>,
    wait: Duration,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE replan_proposals
         SET graph_id = COALESCE($2, graph_id), not_before = now() + make_interval(secs => $3), updated_at = now(),
             status = CASE WHEN status = 'starting' THEN 'pending' ELSE status END
         WHERE id = $1",
    )
    .bind(id)
    .bind(graph_id)
    .bind(wait.as_secs_f64())
    .execute(db)
    .await?;
    Ok(())
}

/// Whether a plan is being drafted on graph `graph_id` (streaming).
pub async fn plan_streaming(db: impl PgExecutor<'_>, graph_id: Uuid) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM plans WHERE graph_id = $1 AND status = 'streaming')",
    )
    .bind(graph_id)
    .fetch_one(db)
    .await
}

/// An issue or project as the planner's instructions name it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub id: Uuid,
    pub kind: ImpactEntityKind,
    pub team_id: Option<Uuid>,
    /// `ENG-14 "Password reset email flow"`, or `project "Customer portal"`.
    pub label: String,
    /// Its workflow state, or a project's status.
    pub state: String,
}

/// The issues and projects among `ids`, named.
pub async fn named(db: impl PgExecutor<'_>, ids: &[Uuid]) -> Result<Vec<Named>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<(Uuid, String, Option<Uuid>, String, String)> = sqlx::query_as(
        "SELECT i.id, 'issue', i.team_id, t.key || '-' || i.number || ' \"' || i.title || '\"',
                s.name
         FROM issues i JOIN teams t ON t.id = i.team_id JOIN issue_states s ON s.id = i.state_id
         WHERE i.id = ANY($1)
         UNION ALL
         SELECT p.id, 'project', p.team_id, 'project \"' || p.name || '\"', p.status
         FROM projects p WHERE p.id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(db)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, kind, team_id, label, state)| Named {
            id,
            kind: if kind == "project" {
                ImpactEntityKind::Project
            } else {
                ImpactEntityKind::Issue
            },
            team_id,
            label,
            state,
        })
        .collect())
}
