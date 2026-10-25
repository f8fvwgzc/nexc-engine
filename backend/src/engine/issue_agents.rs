//! Handing an issue to agents: the graph that plans and executes it, and a
//! plan asked of an agent a comment mentions. Nothing here applies a plan or
//! starts a run: a person still does, as with any plan.

use uuid::Uuid;

use super::{editor, planner};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::graph::{GRAPH_NAME_MAX, GRAPH_TEXT_MAX};
use crate::domain::issue::{AGENT_ASKED_TO_PLAN, Issue, mention_plan_instructions};
use crate::repo;
use crate::repo::issues::MentionedAgent;

/// Gives the issue a graph that plans and executes it: a graph of the
/// issue's team whose goal is the issue's title and description, linked to
/// the issue. Graphs of a team are open to its members only, so only a
/// member of the team can (403 otherwise). Returns the graph's id.
pub async fn create_graph(state: &AppState, actor: Uuid, issue: &Issue) -> Result<Uuid, AppError> {
    let home = editor::graph_home(state, actor, None, Some(issue.team_id))
        .await
        .map_err(|_| {
            AppError::Forbidden("join the team to plan and run its issues as graphs".into())
        })?;
    let name: String = format!("{} {}", issue.identifier, issue.title)
        .chars()
        .take(GRAPH_NAME_MAX)
        .collect();
    let goal = if issue.description.trim().is_empty() {
        issue.title.clone()
    } else {
        format!("{}\n\n{}", issue.title, issue.description.trim())
    };
    let goal: String = goal.chars().take(GRAPH_TEXT_MAX).collect();
    let mut tx = state.db.begin().await?;
    let graph = repo::graphs::create(
        &mut *tx,
        actor,
        home.workspace_id,
        home.team_id,
        &name,
        &format!("Plans and executes {}", issue.identifier),
        &goal,
    )
    .await?;
    repo::issues::link_graph(&mut *tx, issue.id, graph.id).await?;
    tx.commit().await?;
    Ok(graph.id)
}

/// Asks `agent`, which a comment of `actor` mentions, for a plan of the
/// issue, through the planner as `actor`: the issue gets its graph when it
/// has none, and the plan's instructions are the comment as a person reads
/// it, with the agent named to take the steps. The plan streams to the
/// issue's page and waits there to be applied, like one asked for there; an
/// `agent` entry in the timeline says who asked. Returns false, asking
/// nothing, when `actor` is not a member of the issue's team (only members
/// plan a team's work) or when the graph already has a run or a plan under
/// way.
pub async fn ask_to_plan(
    state: &AppState,
    actor: Uuid,
    issue: &Issue,
    agent: &MentionedAgent,
    comment: &str,
) -> Result<bool, AppError> {
    if editor::graph_home(state, actor, None, Some(issue.team_id))
        .await
        .is_err()
    {
        return Ok(false);
    }
    let graph_id = match issue.graph_id {
        Some(id) => id,
        None => create_graph(state, actor, issue).await?,
    };
    if repo::issues::graph_busy(&state.db, graph_id).await? {
        return Ok(false);
    }
    let instructions = mention_plan_instructions(comment, &agent.name, &agent.role);
    planner::start(state, actor, graph_id, instructions).await?;
    repo::issues::record_agent(&state.db, issue.id, actor, &agent.name, AGENT_ASKED_TO_PLAN)
        .await?;
    Ok(true)
}
