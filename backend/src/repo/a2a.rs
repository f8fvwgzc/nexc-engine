//! The A2A adapter's tables: `a2a_counterparts` (remote agents a workspace
//! registered, each with an external agent principal of its own),
//! `a2a_tasks` (the tasks exposed agents received) and the managed graph an
//! exposed agent's inbound tasks run in (`graphs.a2a_agent_id`).

use sqlx::postgres::PgRow;
use sqlx::{FromRow, PgExecutor, Row};
use uuid::Uuid;

use super::enum_col;
use crate::domain::a2a::{A2aCounterpart, A2aTask, Hops, TaskState};
use crate::domain::agent::Agent;
use crate::domain::capabilities::DataLabel;
use crate::domain::run::RunStatus;

/// A counterpart row with its encrypted header, for the gateway only.
#[derive(Debug, Clone)]
pub struct CounterpartRow {
    pub counterpart: A2aCounterpart,
    /// AES-256-GCM under the master key, bound to the counterpart's id.
    pub auth_header: Option<Vec<u8>>,
}

impl FromRow<'_, PgRow> for CounterpartRow {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let auth_header: Option<Vec<u8>> = row.try_get("auth_header")?;
        let allowed: serde_json::Value = row.try_get("allowed_agents")?;
        let card_url: String = row.try_get("card_url")?;
        let host = reqwest::Url::parse(&card_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
            .unwrap_or_default();
        Ok(CounterpartRow {
            counterpart: A2aCounterpart {
                id: row.try_get("id")?,
                workspace_id: row.try_get("workspace_id")?,
                name: row.try_get("name")?,
                card_url,
                host,
                card: row.try_get("card")?,
                has_auth_header: auth_header.is_some(),
                allowed_agents: serde_json::from_value(allowed).unwrap_or_default(),
                max_label: enum_col(row, "max_label")?,
                agent_id: row.try_get("agent_id")?,
                principal_id: row.try_get("principal_id")?,
                last_checked_at: row.try_get("last_checked_at")?,
                last_check_ok: row.try_get("last_check_ok")?,
                last_check_detail: row.try_get("last_check_detail")?,
                created_by: row.try_get("created_by")?,
                created_at: row.try_get("created_at")?,
                updated_at: row.try_get("updated_at")?,
            },
            auth_header,
        })
    }
}

/// The columns of a counterpart row with its principal (SQL stays static).
macro_rules! counterpart_select {
    () => {
        "SELECT c.*, a.principal_id FROM a2a_counterparts c JOIN agents a ON a.id = c.agent_id"
    };
}

/// What a counterpart is registered with.
#[derive(Debug, Clone)]
pub struct NewCounterpart<'a> {
    pub workspace_id: Uuid,
    pub name: &'a str,
    pub card_url: &'a str,
    pub auth_header: Option<&'a [u8]>,
    pub allowed_agents: &'a [Uuid],
    pub max_label: DataLabel,
    pub agent_id: Uuid,
    pub created_by: Uuid,
}

/// Registers a counterpart; the id is chosen by the caller so the header can be bound to it.
pub async fn insert_counterpart(
    db: impl PgExecutor<'_>,
    id: Uuid,
    new: &NewCounterpart<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO a2a_counterparts (id, workspace_id, name, card_url, auth_header, allowed_agents,
                                       max_label, agent_id, created_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(id)
    .bind(new.workspace_id)
    .bind(new.name)
    .bind(new.card_url)
    .bind(new.auth_header)
    .bind(serde_json::to_value(new.allowed_agents).expect("ids serialise"))
    .bind(new.max_label.as_str())
    .bind(new.agent_id)
    .bind(new.created_by)
    .execute(db)
    .await?;
    Ok(())
}

/// The counterparts of a workspace, by name.
pub async fn list_counterparts(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<Vec<CounterpartRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        counterpart_select!(),
        " WHERE c.workspace_id = $1 ORDER BY c.name"
    ))
    .bind(workspace_id)
    .fetch_all(db)
    .await
}

/// One counterpart of a workspace.
pub async fn find_counterpart(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<CounterpartRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        counterpart_select!(),
        " WHERE c.workspace_id = $1 AND c.id = $2"
    ))
    .bind(workspace_id)
    .bind(id)
    .fetch_optional(db)
    .await
}

/// The counterpart of a workspace named `name`.
pub async fn find_counterpart_by_name(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
    name: &str,
) -> Result<Option<CounterpartRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        counterpart_select!(),
        " WHERE c.workspace_id = $1 AND c.name = $2"
    ))
    .bind(workspace_id)
    .bind(name)
    .fetch_optional(db)
    .await
}

/// The counterpart whose principal is the external agent `agent_id`.
pub async fn find_counterpart_by_agent(
    db: impl PgExecutor<'_>,
    agent_id: Uuid,
) -> Result<Option<CounterpartRow>, sqlx::Error> {
    sqlx::query_as(concat!(counterpart_select!(), " WHERE c.agent_id = $1"))
        .bind(agent_id)
        .fetch_optional(db)
        .await
}

/// Records what fetching the card found; the card is replaced only when it was read.
pub async fn set_counterpart_check(
    db: impl PgExecutor<'_>,
    id: Uuid,
    ok: bool,
    detail: &str,
    card: Option<&serde_json::Value>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE a2a_counterparts SET last_checked_at = now(), last_check_ok = $2, last_check_detail = $3,
                card = COALESCE($4::jsonb, card), updated_at = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(ok)
    .bind(detail)
    .bind(card)
    .execute(db)
    .await?;
    Ok(())
}

/// Removes a counterpart with its principal; returns the principal's agent
/// as it was, `None` when the counterpart was not there.
pub async fn delete_counterpart(
    conn: &mut sqlx::PgConnection,
    workspace_id: Uuid,
    id: Uuid,
) -> Result<Option<Agent>, sqlx::Error> {
    let agent: Option<Uuid> = sqlx::query_scalar(
        "DELETE FROM a2a_counterparts WHERE workspace_id = $1 AND id = $2 RETURNING agent_id",
    )
    .bind(workspace_id)
    .bind(id)
    .fetch_optional(&mut *conn)
    .await?;
    let Some(agent) = agent else {
        return Ok(None);
    };
    // The counterpart's agent is NOT NULL and cascades, so it is still there.
    sqlx::query_as("DELETE FROM agents WHERE id = $1 AND workspace_id = $2 RETURNING *")
        .bind(agent)
        .bind(workspace_id)
        .fetch_optional(&mut *conn)
        .await
}

/// How many counterparts a workspace registered.
pub async fn count_counterparts(
    db: impl PgExecutor<'_>,
    workspace_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT count(*) FROM a2a_counterparts WHERE workspace_id = $1")
        .bind(workspace_id)
        .fetch_one(db)
        .await
}

// --- the managed graph of an exposed agent -------------------------------------------------------

/// The graph an exposed agent's inbound tasks run in, created on first use;
/// its owner is the person answerable for the agent (its creator when that
/// account is gone). Returns `(graph id, owner id)`.
pub async fn ensure_graph(
    conn: &mut sqlx::PgConnection,
    agent_id: Uuid,
    workspace_id: Uuid,
    agent_name: &str,
) -> Result<(Uuid, Uuid), sqlx::Error> {
    if let Some(found) =
        sqlx::query_as::<_, (Uuid, Uuid)>("SELECT id, owner_id FROM graphs WHERE a2a_agent_id = $1")
            .bind(agent_id)
            .fetch_optional(&mut *conn)
            .await?
    {
        return Ok(found);
    }
    let owner: Uuid =
        sqlx::query_scalar("SELECT COALESCE(owner_user_id, owner_id) FROM agents WHERE id = $1")
            .bind(agent_id)
            .fetch_one(&mut *conn)
            .await?;
    let id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO graphs (id, owner_id, name, description, goal, workspace_id, a2a_agent_id)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(id)
    .bind(owner)
    .bind(format!("A2A: {agent_name}"))
    .bind("Tasks received over the A2A protocol; one node per task.")
    .bind("Answer the tasks counterpart agents send over A2A.")
    .bind(workspace_id)
    .bind(agent_id)
    .execute(&mut *conn)
    .await?;
    Ok((id, owner))
}

/// The agent every node of a graph is pinned to, when the graph is an
/// exposed agent's A2A graph.
pub async fn pinned_agent_of_graph(
    db: impl PgExecutor<'_>,
    graph_id: Uuid,
) -> Result<Option<Uuid>, sqlx::Error> {
    sqlx::query_scalar("SELECT a2a_agent_id FROM graphs WHERE id = $1")
        .bind(graph_id)
        .fetch_optional(db)
        .await
        .map(Option::flatten)
}

// --- inbound tasks -------------------------------------------------------------------------------

/// A task row with the message as received.
#[derive(Debug, Clone)]
pub struct TaskRow {
    pub task: A2aTask,
    pub message: serde_json::Value,
}

impl FromRow<'_, PgRow> for TaskRow {
    fn from_row(row: &PgRow) -> Result<Self, sqlx::Error> {
        let chain: serde_json::Value = row.try_get("chain")?;
        let message: serde_json::Value = row.try_get("message")?;
        let refused_rule: Option<String> = row.try_get("refused_rule")?;
        let run_status: Option<String> = row.try_get("run_status")?;
        let state = if refused_rule.is_some() {
            TaskState::Failed
        } else {
            run_status
                .as_deref()
                .and_then(|s| s.parse::<RunStatus>().ok())
                .map_or(TaskState::Failed, TaskState::of_run)
        };
        Ok(TaskRow {
            task: A2aTask {
                id: row.try_get("id")?,
                workspace_id: row.try_get("workspace_id")?,
                agent_id: row.try_get("agent_id")?,
                caller_agent_id: row.try_get("caller_agent_id")?,
                caller_principal: row.try_get("caller_principal")?,
                run_id: row.try_get("run_id")?,
                node_id: row.try_get("node_id")?,
                session_id: row.try_get("session_id")?,
                hop: row.try_get("hop")?,
                chain: serde_json::from_value(chain).unwrap_or_default(),
                refused_rule,
                refused_reason: row.try_get("refused_reason")?,
                title: row.try_get("title")?,
                state: state.as_str().to_owned(),
                created_at: row.try_get("created_at")?,
            },
            message,
        })
    }
}

/// The columns of a task row with its title and its run's status (SQL stays static).
macro_rules! task_select {
    () => {
        "SELECT t.*, COALESCE(n.title, '') AS title, r.status AS run_status
         FROM a2a_tasks t LEFT JOIN nodes n ON n.id = t.node_id LEFT JOIN runs r ON r.id = t.run_id"
    };
}

/// What an inbound task is recorded with.
#[derive(Debug, Clone)]
pub struct NewTask<'a> {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub agent_id: Uuid,
    pub caller_agent_id: Option<Uuid>,
    pub caller_principal: &'a str,
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub session_id: Option<&'a str>,
    pub hops: &'a Hops,
    pub refused: Option<(&'a str, &'a str)>,
    pub message: &'a serde_json::Value,
}

/// Records an inbound task.
pub async fn insert_task(db: impl PgExecutor<'_>, new: &NewTask<'_>) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO a2a_tasks (id, workspace_id, agent_id, caller_agent_id, caller_principal, run_id, node_id,
                                session_id, hop, chain, refused_rule, refused_reason, message)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)",
    )
    .bind(new.id)
    .bind(new.workspace_id)
    .bind(new.agent_id)
    .bind(new.caller_agent_id)
    .bind(new.caller_principal)
    .bind(new.run_id)
    .bind(new.node_id)
    .bind(new.session_id)
    .bind(i32::from(new.hops.hop))
    .bind(serde_json::to_value(&new.hops.chain).expect("a chain serialises"))
    .bind(new.refused.map(|(rule, _)| rule))
    .bind(new.refused.map(|(_, reason)| reason))
    .bind(new.message)
    .execute(db)
    .await?;
    Ok(())
}

/// A task by id.
pub async fn find_task(db: impl PgExecutor<'_>, id: Uuid) -> Result<Option<TaskRow>, sqlx::Error> {
    sqlx::query_as(concat!(task_select!(), " WHERE t.id = $1"))
        .bind(id)
        .fetch_optional(db)
        .await
}

/// The tasks an exposed agent received, newest first.
pub async fn list_tasks_for_agent(
    db: impl PgExecutor<'_>,
    agent_id: Uuid,
    limit: i64,
) -> Result<Vec<TaskRow>, sqlx::Error> {
    sqlx::query_as(concat!(
        task_select!(),
        " WHERE t.agent_id = $1 ORDER BY t.id DESC LIMIT $2"
    ))
    .bind(agent_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

/// The hops of the inbound task a run answers, when it answers one.
pub async fn hops_of_run(
    db: impl PgExecutor<'_>,
    run_id: Uuid,
) -> Result<Option<Hops>, sqlx::Error> {
    let row: Option<(i32, serde_json::Value)> =
        sqlx::query_as("SELECT hop, chain FROM a2a_tasks WHERE run_id = $1 LIMIT 1")
            .bind(run_id)
            .fetch_optional(db)
            .await?;
    Ok(row.map(|(hop, chain)| Hops {
        hop: hop.clamp(0, 255) as u8,
        chain: serde_json::from_value(chain).unwrap_or_default(),
    }))
}
