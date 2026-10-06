//! The agent organisation: default org seeding, node → agent assignment,
//! heartbeats and the status report.
#![forbid(unsafe_code)]

pub mod health;

use sqlx::PgConnection;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::agent::{Agent, AgentStatus, DEFAULT_ORG};
use crate::domain::graph::GraphNode;
use crate::domain::ontology::Ontology;
use crate::domain::status::{Backends, OrchestratorStatus};
use crate::engine::credentials;
use crate::repo;
use crate::repo::agents::AgentFields;

/// Creates the default organisation for a user (planner at the top, the
/// specialists reporting to it). Must run inside a transaction.
pub async fn seed_default_org(
    conn: &mut PgConnection,
    owner: Uuid,
    model: &str,
) -> Result<(), sqlx::Error> {
    let mut ids = std::collections::HashMap::new();
    for seed in DEFAULT_ORG {
        let fields = AgentFields {
            name: seed.name.into(),
            role: seed.role.into(),
            title: seed.title.into(),
            model: model.into(),
            system_prompt: seed.system_prompt.into(),
            reports_to: seed.reports_to.and_then(|role| ids.get(role).copied()),
            budget_tokens: seed.budget_tokens,
            runtime: seed.runtime,
            status: AgentStatus::Active,
        };
        let agent = repo::agents::create(&mut *conn, owner, &fields).await?;
        ids.insert(seed.role, agent.id);
    }
    Ok(())
}

/// Seeds the default organisation for every user that has no agents yet.
pub async fn seed_missing_orgs(state: &AppState) -> anyhow::Result<()> {
    for user in repo::users::all_ids(&state.db).await? {
        if !repo::agents::exists_for(&state.db, user).await? {
            let mut tx = state.db.begin().await?;
            seed_default_org(&mut tx, user, &state.settings.llm_model).await?;
            tx.commit().await?;
        }
    }
    Ok(())
}

/// The agent responsible for a node: by its `agent_role`, else by the
/// default role of its type in `ontology`.
pub async fn assign(
    state: &AppState,
    owner: Uuid,
    node: &GraphNode,
    ontology: &Ontology,
) -> Result<Option<Agent>, sqlx::Error> {
    let type_role = ontology.role_for(&node.kind);
    let role = node
        .agent_role
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .unwrap_or(type_role);
    match repo::agents::find_by_role(&state.db, owner, role).await? {
        Some(agent) => Ok(Some(agent)),
        None => repo::agents::find_by_role(&state.db, owner, type_role).await,
    }
}

/// One heartbeat tick: marks busy agents alive and flags exhausted budgets.
pub async fn heartbeat(state: &AppState) -> anyhow::Result<()> {
    let busy = state.engine.busy_agents();
    if !busy.is_empty() {
        repo::agents::heartbeat(&state.db, &busy).await?;
    }
    let flagged = repo::agents::flag_over_budget(&state.db).await?;
    if flagged > 0 {
        tracing::info!(flagged, "agents reached their token budget");
    }
    Ok(())
}

/// Status report for `owner`.
pub async fn status(state: &AppState, owner: Uuid) -> Result<OrchestratorStatus, AppError> {
    let (queue_depth, running_nodes, active_runs) = repo::runs::activity(&state.db, owner).await?;
    let agents_active = repo::agents::count_active(&state.db, owner).await?;
    let llm = credentials::resolve(state, owner, None).await?;
    Ok(OrchestratorStatus {
        demo_mode: llm.is_demo(),
        queue_depth,
        running_nodes,
        active_runs,
        agents_active,
        backends: Backends {
            llm: health::llm(&llm),
            agent_runtime: state.health.agent_runtime(state).await,
            symphony: state.health.symphony(state).await,
        },
    })
}
