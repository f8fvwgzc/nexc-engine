//! Runs of external agents (the Agent Firewall API): each external agent
//! owns one managed graph of its workspace; every run it reports, and the
//! session its decisions are recorded in, is a run of that graph with one
//! node, so the trace, the audit and the console treat it like any other.

use uuid::Uuid;

use crate::domain::run::RunStatus;
use crate::domain::seal::SignedSeal;

/// The managed graph of an external agent, created on first use. The owner
/// is the person answerable for the agent (its creator when that account
/// is gone).
pub async fn ensure_graph(
    conn: &mut sqlx::PgConnection,
    agent_id: Uuid,
    workspace_id: Uuid,
    agent_name: &str,
) -> Result<(Uuid, Uuid), sqlx::Error> {
    if let Some(found) = sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT id, owner_id FROM graphs WHERE external_agent_id = $1 AND deleted_at IS NULL",
    )
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
    // Two first uses at once both find no graph: the later insert gives way to the earlier one
    // (`graphs_external_agent_live_key`) and reads it back.
    let created: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO graphs (id, owner_id, name, description, goal, workspace_id, external_agent_id)
         VALUES ($1, $2, $3, $4, '', $5, $6)
         ON CONFLICT DO NOTHING RETURNING id",
    )
    .bind(id)
    .bind(owner)
    .bind(format!("External agent: {agent_name}"))
    .bind("Runs reported through the Agent Firewall API; one node per run.")
    .bind(workspace_id)
    .bind(agent_id)
    .fetch_optional(&mut *conn)
    .await?;
    if created.is_some() {
        return Ok((id, owner));
    }
    sqlx::query_as::<_, (Uuid, Uuid)>(
        "SELECT id, owner_id FROM graphs WHERE external_agent_id = $1 AND deleted_at IS NULL",
    )
    .bind(agent_id)
    .fetch_one(&mut *conn)
    .await
}

/// Opens a run of an external agent: a node for it, a run already running
/// (no dispatcher picks it up), one node run. Returns `(run id, node id)`.
pub async fn open_run(
    conn: &mut sqlx::PgConnection,
    graph_id: Uuid,
    owner_id: Uuid,
    agent_id: Uuid,
    agent_role: &str,
    title: &str,
    external_run_id: &str,
) -> Result<(Uuid, Uuid), sqlx::Error> {
    let node_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO nodes (id, graph_id, title, content, kind, agent_role, executor, status, external_run_id)
         VALUES ($1, $2, $3, '', 'task', $4, 'agent', 'running', $5)",
    )
    .bind(node_id)
    .bind(graph_id)
    .bind(title)
    .bind(agent_role)
    .bind(external_run_id)
    .execute(&mut *conn)
    .await?;
    let run_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO runs (id, graph_id, owner_id, status, max_concurrency, started_at)
         VALUES ($1, $2, $3, 'running', 1, now())",
    )
    .bind(run_id)
    .bind(graph_id)
    .bind(owner_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO node_runs (run_id, node_id, position, status, attempt, executor, agent_id, started_at)
         VALUES ($1, $2, 0, 'running', 1, 'agent', $3, now())",
    )
    .bind(run_id)
    .bind(node_id)
    .bind(agent_id)
    .execute(&mut *conn)
    .await?;
    Ok((run_id, node_id))
}

/// Closes an external run: the node, its run and the seal of its trace,
/// signed (`seal`) when there is one.
pub async fn close_run(
    db: &mut sqlx::PgConnection,
    run_id: Uuid,
    node_id: Uuid,
    status: RunStatus,
    output: Option<&str>,
    trace_hash: Option<&[u8]>,
    seal: Option<&SignedSeal>,
) -> Result<(), sqlx::Error> {
    let node_status = match status {
        RunStatus::Succeeded => "succeeded",
        RunStatus::Cancelled => "cancelled",
        _ => "failed",
    };
    sqlx::query(
        "UPDATE node_runs SET status = $3, output = $4, finished_at = now() WHERE run_id = $1 AND node_id = $2",
    )
    .bind(run_id)
    .bind(node_id)
    .bind(node_status)
    .bind(output)
    .execute(&mut *db)
    .await?;
    sqlx::query("UPDATE nodes SET status = $2, output = $3, updated_at = now() WHERE id = $1")
        .bind(node_id)
        .bind(node_status)
        .bind(output)
        .execute(&mut *db)
        .await?;
    sqlx::query(
        "UPDATE runs SET status = $2, finished_at = now(), trace_hash = $3,
                         seal_signed = $4, seal_signature = $5, seal_kid = $6
         WHERE id = $1",
    )
    .bind(run_id)
    .bind(status.as_str())
    .bind(trace_hash)
    .bind(seal.map(|s| s.signed.as_slice()))
    .bind(seal.map(|s| s.signature.as_slice()))
    .bind(seal.map(|s| s.kid.as_str()))
    .execute(&mut *db)
    .await?;
    Ok(())
}
