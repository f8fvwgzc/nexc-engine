//! Agents of each user's organisation.

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::agent::{Agent, AgentOwner, AgentRuntime, AgentStatus, AgentTeam, principal_id};
use crate::domain::capabilities::Capabilities;

impl FromRow<'_, PgRow> for Agent {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let capabilities: serde_json::Value = row.try_get("capabilities")?;
        let capabilities =
            serde_json::from_value(capabilities).map_err(|e| sqlx::Error::ColumnDecode {
                index: "capabilities".into(),
                source: Box::new(e),
            })?;
        Ok(Agent {
            id: row.try_get("id")?,
            principal_id: row.try_get("principal_id")?,
            name: row.try_get("name")?,
            role: row.try_get("role")?,
            title: row.try_get("title")?,
            model: row.try_get("model")?,
            system_prompt: row.try_get("system_prompt")?,
            reports_to: row.try_get("reports_to")?,
            budget_tokens: row.try_get("budget_tokens")?,
            spent_tokens: row.try_get("spent_tokens")?,
            status: enum_col(row, "status")?,
            runtime: enum_col(row, "runtime")?,
            heartbeat_at: row.try_get("heartbeat_at")?,
            created_at: row.try_get("created_at")?,
            owner_user_id: row.try_get("owner_user_id")?,
            team_id: row.try_get("team_id")?,
            purpose: row.try_get("purpose")?,
            capabilities,
            policy_version: row.try_get("policy_version")?,
            quarantined_at: row.try_get("quarantined_at")?,
            risk_score: row.try_get("risk_score")?,
            risk_reasons: serde_json::from_value(
                row.try_get::<serde_json::Value, _>("risk_reasons")?,
            )
            .unwrap_or_default(),
            risk_updated_at: row.try_get("risk_updated_at")?,
            a2a_exposed: row.try_get("a2a_exposed")?,
        })
    }
}

/// Records what the agent's latest run scored against its baseline.
pub async fn set_risk(
    db: impl PgExecutor<'_>,
    id: Uuid,
    score: f64,
    reasons: &[String],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE agents SET risk_score = $2, risk_reasons = $3, risk_updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .bind(score)
    .bind(serde_json::to_value(reasons).expect("reasons serialise"))
    .execute(db)
    .await?;
    Ok(())
}

/// Fields of an agent to insert or fully replace.
#[derive(Debug, Clone)]
pub struct AgentFields {
    pub name: String,
    pub role: String,
    pub title: String,
    pub model: String,
    pub system_prompt: String,
    pub reports_to: Option<Uuid>,
    pub budget_tokens: i64,
    pub runtime: AgentRuntime,
    pub status: AgentStatus,
    pub owner_user_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub purpose: String,
    pub capabilities: Capabilities,
    /// Whether the agent is published as an A2A agent.
    pub a2a_exposed: bool,
}

impl AgentFields {
    /// The fields of `agent` as they are.
    pub fn of(agent: &Agent) -> Self {
        AgentFields {
            name: agent.name.clone(),
            role: agent.role.clone(),
            title: agent.title.clone(),
            model: agent.model.clone(),
            system_prompt: agent.system_prompt.clone(),
            reports_to: agent.reports_to,
            budget_tokens: agent.budget_tokens,
            runtime: agent.runtime,
            status: agent.status,
            owner_user_id: agent.owner_user_id,
            team_id: agent.team_id,
            purpose: agent.purpose.clone(),
            capabilities: agent.capabilities.clone(),
            a2a_exposed: agent.a2a_exposed,
        }
    }
}

/// Agents of a workspace, in creation order.
pub async fn list(db: impl PgExecutor<'_>, workspace_id: Uuid) -> Result<Vec<Agent>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM agents WHERE workspace_id = $1 ORDER BY created_at, id")
        .bind(workspace_id)
        .fetch_all(db)
        .await
}

/// One agent of a workspace.
pub async fn find(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<Agent>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM agents WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .fetch_optional(db)
        .await
}

/// The agent of a workspace with `principal` (`agent://{workspace}/{name}`).
pub async fn find_by_principal(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    principal: &str,
) -> Result<Option<Agent>, sqlx::Error> {
    sqlx::query_as("SELECT * FROM agents WHERE workspace_id = $1 AND principal_id = $2")
        .bind(workspace_id)
        .bind(principal)
        .fetch_optional(db)
        .await
}

/// The agent with `role`, preferring active ones, then the oldest.
pub async fn find_by_role(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    role: &str,
) -> Result<Option<Agent>, sqlx::Error> {
    sqlx::query_as(
        "SELECT * FROM agents WHERE workspace_id = $1 AND role = $2
         ORDER BY (status = 'active') DESC, created_at LIMIT 1",
    )
    .bind(workspace_id)
    .bind(role)
    .fetch_optional(db)
    .await
}

/// Inserts an agent into a workspace; `owner_id` records who created it.
pub async fn create(
    db: impl PgExecutor<'_>,
    owner_id: Uuid,
    workspace_id: Uuid,
    f: &AgentFields,
) -> Result<Agent, sqlx::Error> {
    let capabilities = serde_json::to_value(&f.capabilities).expect("capabilities serialise");
    sqlx::query_as(
        "INSERT INTO agents (id, owner_id, name, role, title, model, system_prompt, reports_to, budget_tokens,
                             runtime, status, workspace_id, principal_id, owner_user_id, team_id, purpose,
                             capabilities, a2a_exposed)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18) RETURNING *",
    )
    .bind(Uuid::now_v7())
    .bind(owner_id)
    .bind(&f.name)
    .bind(&f.role)
    .bind(&f.title)
    .bind(&f.model)
    .bind(&f.system_prompt)
    .bind(f.reports_to)
    .bind(f.budget_tokens)
    .bind(f.runtime.as_str())
    .bind(f.status.as_str())
    .bind(workspace_id)
    .bind(principal_id(Some(workspace_id), owner_id, &f.name))
    .bind(f.owner_user_id.or(Some(owner_id)))
    .bind(f.team_id)
    .bind(&f.purpose)
    .bind(capabilities)
    .bind(f.a2a_exposed)
    .fetch_one(db)
    .await
}

/// Replaces the editable fields of an agent. The principal follows the
/// name; a change of capabilities raises the policy version.
pub async fn update(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
    f: &AgentFields,
) -> Result<Option<Agent>, sqlx::Error> {
    let capabilities = serde_json::to_value(&f.capabilities).expect("capabilities serialise");
    sqlx::query_as(
        "UPDATE agents SET name = $3, role = $4, title = $5, model = $6, system_prompt = $7, reports_to = $8,
                budget_tokens = $9, runtime = $10, status = $11, updated_at = now(),
                principal_id = 'agent://' || workspace_id::text || '/' || $3,
                owner_user_id = $12, team_id = $13, purpose = $14,
                policy_version = policy_version + CASE WHEN capabilities = $15::jsonb THEN 0 ELSE 1 END,
                capabilities = $15, a2a_exposed = $16
         WHERE workspace_id = $1 AND id = $2 RETURNING *",
    )
    .bind(workspace_id)
    .bind(id)
    .bind(&f.name)
    .bind(&f.role)
    .bind(&f.title)
    .bind(&f.model)
    .bind(&f.system_prompt)
    .bind(f.reports_to)
    .bind(f.budget_tokens)
    .bind(f.runtime.as_str())
    .bind(f.status.as_str())
    .bind(f.owner_user_id)
    .bind(f.team_id)
    .bind(&f.purpose)
    .bind(capabilities)
    .bind(f.a2a_exposed)
    .fetch_optional(db)
    .await
}

/// Takes an agent out of service: status `quarantined`, the policy version
/// raised. `None` when it is already quarantined or unknown.
pub async fn quarantine(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<Agent>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE agents SET status = 'quarantined', quarantined_at = now(),
                policy_version = policy_version + 1, updated_at = now()
         WHERE workspace_id = $1 AND id = $2 AND status <> 'quarantined' RETURNING *",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_optional(db)
    .await
}

/// Puts a quarantined agent back in service (active, or over budget when
/// its budget is spent). `None` when it is not quarantined or unknown.
pub async fn release(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<Agent>, sqlx::Error> {
    sqlx::query_as(
        "UPDATE agents SET status = CASE WHEN budget_tokens > 0 AND spent_tokens >= budget_tokens
                                         THEN 'over_budget' ELSE 'active' END,
                quarantined_at = NULL, policy_version = policy_version + 1, updated_at = now()
         WHERE workspace_id = $1 AND id = $2 AND status = 'quarantined' RETURNING *",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_optional(db)
    .await
}

/// Who answers for an agent, if their account still exists.
pub async fn owner(
    db: impl PgExecutor<'_>,
    user_id: Option<Uuid>,
) -> Result<Option<AgentOwner>, sqlx::Error> {
    let Some(user_id) = user_id else {
        return Ok(None);
    };
    let row: Option<(Uuid, String)> = sqlx::query_as("SELECT id, name FROM users WHERE id = $1")
        .bind(user_id)
        .fetch_optional(db)
        .await?;
    Ok(row.map(|(id, name)| AgentOwner { id, name }))
}

/// The team an agent belongs to, if it still exists.
pub async fn team(
    db: impl PgExecutor<'_>,
    team_id: Option<Uuid>,
) -> Result<Option<AgentTeam>, sqlx::Error> {
    let Some(team_id) = team_id else {
        return Ok(None);
    };
    let row: Option<(Uuid, String, String)> =
        sqlx::query_as("SELECT id, name, key FROM teams WHERE id = $1")
            .bind(team_id)
            .fetch_optional(db)
            .await?;
    Ok(row.map(|(id, name, key)| AgentTeam { id, name, key }))
}

/// Whether `team_id` is a team of `workspace_id`.
pub async fn team_in_workspace(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    team_id: Uuid,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM teams WHERE id = $1 AND workspace_id = $2)")
        .bind(team_id)
        .bind(workspace_id)
        .fetch_one(db)
        .await
}

/// Deletes an agent (reports re-parent to nobody). Returns false if absent.
pub async fn delete(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let done = sqlx::query("DELETE FROM agents WHERE workspace_id = $1 AND id = $2")
        .bind(workspace_id)
        .bind(id)
        .execute(db)
        .await?;
    Ok(done.rows_affected() == 1)
}

/// Adds spent tokens and records a heartbeat (the agent just finished work);
/// flags the agent `over_budget` when it crosses its budget.
pub async fn add_spent(db: impl PgExecutor<'_>, id: Uuid, tokens: i64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE agents SET spent_tokens = spent_tokens + $2,
                status = CASE WHEN budget_tokens > 0 AND spent_tokens + $2 >= budget_tokens AND status = 'active'
                              THEN 'over_budget' ELSE status END,
                heartbeat_at = now(), updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(tokens)
    .execute(db)
    .await?;
    Ok(())
}

/// Updates `heartbeat_at` of the given agents.
pub async fn heartbeat(db: impl PgExecutor<'_>, ids: &[Uuid]) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE agents SET heartbeat_at = now() WHERE id = ANY($1)")
        .bind(ids)
        .execute(db)
        .await?;
    Ok(())
}

/// Flags every active agent that reached its budget; returns how many changed.
pub async fn flag_over_budget(db: impl PgExecutor<'_>) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE agents SET status = 'over_budget', updated_at = now()
         WHERE status = 'active' AND budget_tokens > 0 AND spent_tokens >= budget_tokens",
    )
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}

/// Active agents in the workspaces `user_id` belongs to.
pub async fn count_active(db: impl PgExecutor<'_>, user_id: Uuid) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT count(*) FROM agents a JOIN workspace_members m ON m.workspace_id = a.workspace_id
         WHERE m.user_id = $1 AND a.status = 'active'",
    )
    .bind(user_id)
    .fetch_one(db)
    .await
}

/// The workspace an agent belongs to. The outer `None` is an unknown agent,
/// the inner one a legacy agent that was never adopted by a workspace.
pub async fn workspace_of(
    db: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<Option<Uuid>>, sqlx::Error> {
    sqlx::query_scalar("SELECT workspace_id FROM agents WHERE id = $1")
        .bind(id)
        .fetch_optional(db)
        .await
}

/// Workspaces that have no agents yet, with who created them.
pub async fn workspaces_without_agents(
    db: impl PgExecutor<'_>,
) -> Result<Vec<(Uuid, Option<Uuid>)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT w.id, w.created_by FROM workspaces w
         WHERE NOT EXISTS (SELECT 1 FROM agents a WHERE a.workspace_id = w.id)
         ORDER BY w.created_at",
    )
    .fetch_all(db)
    .await
}

/// Creators of agents that predate workspaces, oldest account first.
pub async fn orphan_owners(db: impl PgExecutor<'_>) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT u.id FROM users u
         WHERE EXISTS (SELECT 1 FROM agents a WHERE a.owner_id = u.id AND a.workspace_id IS NULL)
         ORDER BY u.created_at",
    )
    .fetch_all(db)
    .await
}

/// Moves the legacy agents of `owner_id` into their first workspace, except
/// those whose name is already taken there (they stay unassigned and unused).
/// Their principal is renamed after the workspace.
pub async fn adopt_orphans_of(db: impl PgExecutor<'_>, owner_id: Uuid) -> Result<u64, sqlx::Error> {
    let done = sqlx::query(
        "UPDATE agents a SET workspace_id = w.workspace_id,
                principal_id = 'agent://' || w.workspace_id::text || '/' || a.name
         FROM (SELECT workspace_id FROM workspace_members
               WHERE user_id = $1 AND role <> 'guest'
               ORDER BY created_at, workspace_id LIMIT 1) w
         WHERE a.owner_id = $1 AND a.workspace_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM agents b
                           WHERE b.workspace_id = w.workspace_id AND b.name = a.name)",
    )
    .bind(owner_id)
    .execute(db)
    .await?;
    Ok(done.rows_affected())
}
