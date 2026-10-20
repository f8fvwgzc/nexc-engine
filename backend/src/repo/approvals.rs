//! Approvals (`approvals`): asked for by a run, decided once.

use chrono::{DateTime, Utc};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::approval::{Approval, ApprovalStatus, Decider};
use crate::domain::policy::PolicyVersions;

impl FromRow<'_, PgRow> for Approval {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let decided_by: Option<Uuid> = row.try_get("decided_by")?;
        let decided_by_name: String = row.try_get("decided_by_name")?;
        let decided_at: Option<DateTime<Utc>> = row.try_get("decided_at")?;
        let versions: serde_json::Value = row.try_get("policy_versions")?;
        Ok(Approval {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            run_id: row.try_get("run_id")?,
            node_id: row.try_get("node_id")?,
            graph_id: row.try_get("graph_id")?,
            agent_id: row.try_get("agent_id")?,
            principal: row.try_get("principal")?,
            span_id: row.try_get("span_id")?,
            tool: row.try_get("tool")?,
            args_sha256: row.try_get("args_sha256")?,
            rule: row.try_get("rule")?,
            reason: row.try_get("reason")?,
            approver_ids: row.try_get("approver_ids")?,
            status: enum_col(row, "status")?,
            requested_at: row.try_get("requested_at")?,
            expires_at: row.try_get("expires_at")?,
            decided_by: decided_at.is_some().then_some(Decider {
                id: decided_by,
                name: decided_by_name,
            }),
            decided_at,
            decision_reason: row.try_get("decision_reason")?,
            decision_span_id: row.try_get("decision_span_id")?,
            policy_versions: serde_json::from_value(versions).unwrap_or_default(),
        })
    }
}

/// What a run asks for.
#[derive(Debug, Clone)]
pub struct ApprovalRequest<'a> {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub graph_id: Uuid,
    pub agent_id: Option<Uuid>,
    pub principal: &'a str,
    pub span_id: Uuid,
    pub tool: &'a str,
    pub args_sha256: Option<&'a str>,
    pub rule: &'a str,
    pub reason: &'a str,
    pub approver_ids: &'a [Uuid],
    pub expires_at: DateTime<Utc>,
    pub policy_versions: &'a PolicyVersions,
}

/// Records a pending approval.
pub async fn insert(
    db: impl PgExecutor<'_>,
    r: &ApprovalRequest<'_>,
) -> Result<Approval, sqlx::Error> {
    sqlx::query_as(
        "INSERT INTO approvals (id, workspace_id, run_id, node_id, graph_id, agent_id, principal, span_id,
                                tool, args_sha256, rule, reason, approver_ids, expires_at, policy_versions)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
         RETURNING *",
    )
    .bind(r.id)
    .bind(r.workspace_id)
    .bind(r.run_id)
    .bind(r.node_id)
    .bind(r.graph_id)
    .bind(r.agent_id)
    .bind(r.principal)
    .bind(r.span_id)
    .bind(r.tool)
    .bind(r.args_sha256)
    .bind(r.rule)
    .bind(r.reason)
    .bind(r.approver_ids)
    .bind(r.expires_at)
    .bind(serde_json::to_value(r.policy_versions).expect("versions serialise"))
    .fetch_one(db)
    .await
}

/// One approval by id.
pub async fn find(db: impl PgExecutor<'_>, id: Uuid) -> Result<Option<Approval>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM approvals WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Decides a pending approval; `None` when it was not pending (or unknown).
pub async fn decide(
    db: impl PgExecutor<'_>,
    id: Uuid,
    status: ApprovalStatus,
    decided_by: Uuid,
    reason: &str,
) -> Result<Option<Approval>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE approvals SET status = $2, decided_by = $3,
                decided_by_name = COALESCE((SELECT name FROM users WHERE id = $3), ''),
                decision_reason = $4, decided_at = now()
         WHERE id = $1 AND status = 'pending' RETURNING *",
    )
    .bind(id)
    .bind(status.as_str())
    .bind(decided_by)
    .bind(reason)
    .fetch_optional(db)
    .await
}

/// The latest approval a node of a run asked for (the one a node waiting
/// when its run was taken over waits for).
pub async fn latest_of_node(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    node_id: Uuid,
) -> Result<Option<Approval>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM approvals WHERE run_id = $1 AND node_id = $2
         ORDER BY requested_at DESC, id DESC LIMIT 1",
    )
    .bind(run_id)
    .bind(node_id)
    .fetch_optional(db)
    .await
}

/// Decides a pending approval as `from` was decided: the request an
/// attempt asks again after a takeover gets the decision already taken on
/// the interrupted attempt's request, by the same person, for the same
/// reason, as long as that person may still decide it (one of its
/// approvers, or an admin of its workspace). `None` when it was not pending
/// or the decider may not decide it: the approvers are then asked.
pub async fn carry(
    db: impl PgExecutor<'_>,
    id: Uuid,
    from: Uuid,
) -> Result<Option<Approval>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE approvals a SET status = o.status, decided_by = o.decided_by,
                decided_by_name = o.decided_by_name, decision_reason = o.decision_reason,
                decided_at = now()
         FROM approvals o
         WHERE a.id = $1 AND o.id = $2 AND a.status = 'pending' AND o.status IN ('approved', 'denied')
           AND o.decided_by IS NOT NULL
           AND (o.decided_by = ANY(a.approver_ids)
                OR EXISTS (SELECT 1 FROM workspace_members m
                           WHERE m.workspace_id = a.workspace_id AND m.user_id = o.decided_by
                             AND m.role IN ('owner', 'admin')))
         RETURNING a.*",
    )
    .bind(id)
    .bind(from)
    .fetch_optional(db)
    .await
}

/// Marks a still-pending approval expired (the call stopped waiting); returns whether it did.
pub async fn expire(db: impl PgExecutor<'_>, id: Uuid) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE approvals SET status = 'expired', decided_at = now()
         WHERE id = $1 AND status = 'pending'",
    )
    .bind(id)
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// Notes the `approval` span that recorded the decision.
pub async fn set_decision_span(
    db: impl PgExecutor<'_>,
    id: Uuid,
    span_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE approvals SET decision_span_id = $2 WHERE id = $1")
        .bind(id)
        .bind(span_id)
        .execute(db)
        .await?;
    Ok(())
}

/// Notes the `approval` span that recorded the decision unless one is noted
/// already; the span id on the record either way. Whoever notes it first
/// (the instance holding the external agent's run, or the one answering
/// its `AwaitApproval`) names the span every signed copy of the decision
/// points at.
pub async fn claim_decision_span(
    db: &mut sqlx::PgConnection,
    id: Uuid,
    span_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    let claimed: Option<Option<Uuid>> = sqlx::query_scalar(
        "UPDATE approvals SET decision_span_id = $2
         WHERE id = $1 AND status <> 'pending' AND decision_span_id IS NULL
         RETURNING decision_span_id",
    )
    .bind(id)
    .bind(span_id)
    .fetch_optional(&mut *db)
    .await?;
    if let Some(claimed) = claimed {
        return Ok(claimed);
    }
    // Noted already (a statement of its own: it sees a claim that just committed).
    sqlx::query_scalar(
        "SELECT decision_span_id FROM approvals WHERE id = $1 AND status <> 'pending'",
    )
    .bind(id)
    .fetch_optional(&mut *db)
    .await
    .map(Option::flatten)
}

/// Uses an approval granted to external agent `agent_id` through the
/// firewall for `tool` on the reported call `span_id`: once, only if it was
/// granted before it expired, and only with the same arguments when the
/// approval names their digest. Whether it was granted and is now used.
pub async fn consume(
    db: impl PgExecutor<'_>,
    id: Uuid,
    agent_id: Uuid,
    tool: &str,
    args_sha256: Option<&str>,
    span_id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE approvals SET consumed_span_id = $5
         WHERE id = $1 AND agent_id = $2 AND tool = $3 AND status = 'approved'
           AND consumed_span_id IS NULL AND decided_at <= expires_at
           AND (args_sha256 IS NULL OR args_sha256 = $4::text)
           AND graph_id IN (SELECT id FROM graphs WHERE external_agent_id = $2)",
    )
    .bind(id)
    .bind(agent_id)
    .bind(tool)
    .bind(args_sha256)
    .bind(span_id)
    .execute(db)
    .await?;
    Ok(done.rows_affected() == 1)
}

/// An approval external agent `agent_id` asked for through the firewall
/// (one in its own graph: never a native run's).
pub async fn find_external(
    db: impl PgExecutor<'_>,
    id: Uuid,
    agent_id: Uuid,
) -> Result<Option<Approval>, sqlx::Error> {
    sqlx::query_as(
        "SELECT a.* FROM approvals a JOIN graphs g ON g.id = a.graph_id
         WHERE a.id = $1 AND a.agent_id = $2 AND g.external_agent_id = $2",
    )
    .bind(id)
    .bind(agent_id)
    .fetch_optional(db)
    .await
}

/// The same request still pending in the same run (the agent asked again
/// before anyone decided): it is answered by that approval.
pub async fn find_pending_like(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
    agent_id: Uuid,
    tool: &str,
    args_sha256: Option<&str>,
) -> Result<Option<Approval>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM approvals
         WHERE run_id = $1 AND agent_id = $2 AND tool = $3 AND status = 'pending'
           AND args_sha256 IS NOT DISTINCT FROM $4::text AND expires_at > now()
         ORDER BY requested_at DESC, id DESC LIMIT 1",
    )
    .bind(run_id)
    .bind(agent_id)
    .bind(tool)
    .bind(args_sha256)
    .fetch_optional(db)
    .await
}

/// How many approvals of `agent_id` are pending.
pub async fn pending_of_agent(db: impl PgExecutor<'_>, agent_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM approvals WHERE agent_id = $1 AND status = 'pending'")
        .bind(agent_id)
        .fetch_one(db)
        .await
}

/// Expires the approvals external agents asked for through the firewall
/// that are past their time (the instance that watched them may be gone);
/// the ones it expired.
pub async fn expire_overdue_external(
    db: impl PgExecutor<'_>,
) -> Result<Vec<Approval>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE approvals SET status = 'expired', decided_at = now()
         WHERE status = 'pending' AND expires_at < now()
           AND graph_id IN (SELECT id FROM graphs WHERE external_agent_id IS NOT NULL)
         RETURNING *",
    )
    .fetch_all(db)
    .await
}

/// Approvals of a workspace, newest first; `status` narrows the list.
pub async fn list(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    status: Option<ApprovalStatus>,
    limit: i64,
) -> Result<Vec<Approval>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM approvals
         WHERE workspace_id = $1 AND ($2::text IS NULL OR status = $2)
         ORDER BY requested_at DESC, id DESC LIMIT $3",
    )
    .bind(workspace_id)
    .bind(status.map(|s| s.as_str()))
    .bind(limit)
    .fetch_all(db)
    .await
}

/// Approvals asked for during a run, oldest first.
pub async fn list_for_run(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
) -> Result<Vec<Approval>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM approvals WHERE run_id = $1 ORDER BY requested_at, id")
        .bind(run_id)
        .fetch_all(db)
        .await
}

/// Puts an approval to decide into the inbox of each of `user_ids`.
pub async fn notify(
    db: impl PgExecutor<'_>,
    approval_id: Uuid,
    workspace_id: Uuid,
    user_ids: &[Uuid],
) -> Result<(), sqlx::Error> {
    if user_ids.is_empty() {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO notifications (id, user_id, workspace_id, approval_id, kind)
         SELECT gen_random_uuid(), u, $2, $1, 'approval' FROM unnest($3::uuid[]) AS u",
    )
    .bind(approval_id)
    .bind(workspace_id)
    .bind(user_ids)
    .execute(db)
    .await?;
    Ok(())
}
