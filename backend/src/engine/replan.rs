//! Re-plan proposals: when the change-impact engine recommends a partial or
//! full re-plan, nexc's planner drafts one, and a person applies it.
//!
//! The assessment is taken up in the transaction that writes the change
//! (`engine::impact`, `repo::replan::propose`): at most one open proposal per
//! re-plan root, a newer assessment of the same root extending a waiting one
//! or superseding one already started. Once that transaction commits, this
//! module starts the planner ([`planner::start`]) — after the proposal waited
//! `NEXC_REPLAN_DEBOUNCE_SECS` without a further change to its root, so a
//! burst of edits starts one LLM run, and never while a plan is still being
//! drafted on the same graph.
//!
//! What is planned: the re-plan root's graph. nexc plans at the level of an
//! issue — an issue's graph plans and executes it (`POST /issues/{iid}/graph`)
//! — so a root issue without a graph is given one, as asking an agent for a
//! plan in a comment does (`engine::issue_agents`): an empty graph of the
//! issue's team, whose goal is what the issue reads now. A project has no
//! graph, and its issues may span many: re-planning one would either fan out
//! into an LLM run per issue (what the debounce exists to prevent) or draft a
//! plan that maps to no issue. So a project root is skipped with that reason;
//! the owners of its affected issues are told in their inbox what each issue
//! needs, and each can ask for a plan on its page.
//!
//! The planner runs as the person who made the change (their LLM key, their
//! workspace's guardrails) and only on a graph they may work on; a change
//! nobody made (a run moving its issue) proposes nothing of its own. Its
//! instructions are the assessment as every member of the graph's team may
//! read it ([`crate::domain::impact::replan_instructions`]): items of other
//! teams are left out. The plan then waits on the graph, `ready`, for someone
//! to review and apply (`POST /graphs/{gid}/plans/{pid}/apply`): nothing here
//! applies a plan or starts a run.

use std::collections::HashMap;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{issue_agents, planner};
use crate::app::AppState;
use crate::domain::impact::{
    ImpactEntityKind, NewAssessment, REPLAN_INSTRUCTIONS_MAX, ReplanSeed, replan_instructions,
};
use crate::domain::{AppError, error_chain};
use crate::repo;
use crate::repo::replan::Claimed;

/// How often waiting proposals are looked for besides being woken: those another backend took
/// up, and those left behind by one that stopped.
pub const SWEEP: Duration = Duration::from_secs(30);
/// Proposals claimed at a time.
const CLAIM_BATCH: i64 = 8;
/// The shortest wait between two looks, so a proposal another backend holds is not polled hot.
const MIN_WAIT: Duration = Duration::from_millis(200);
/// How long a proposal waits, at least, when its graph is drafting another plan.
const BUSY_WAIT: Duration = Duration::from_secs(5);

/// Why a project root drafts no plan.
pub const PROJECT_HAS_NO_GRAPH: &str = "a project has no graph to re-plan, and its issues may span \
    many: the owners of its affected issues are told what each needs, and each can ask for a plan \
    on the issue's page";

/// Wakes this backend's worker after a change that took up a re-plan committed:
/// the proposal is visible now, and starts once its debounce is over.
pub fn wake(state: &AppState, assessed: Option<&NewAssessment>) {
    if assessed.is_some_and(|a| a.replan_id.is_some()) {
        state.replans.notify_one();
    }
}

/// Starts the planner for every proposal whose debounce is over, when woken
/// ([`wake`]) or at the latest when the next one is due (every [`SWEEP`]
/// otherwise), until `shutdown`. Off with `NEXC_REPLAN_PROPOSALS=false`.
pub async fn worker(state: AppState, shutdown: CancellationToken) {
    if !state.settings.replan_proposals {
        return;
    }
    loop {
        let wait = match start_due(&state).await {
            Ok(next) => next.unwrap_or(SWEEP).clamp(MIN_WAIT, SWEEP),
            Err(err) => {
                tracing::warn!(error = %error_chain(&err), "re-plan proposals could not be started; trying again");
                SWEEP
            }
        };
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = state.replans.notified() => {}
            () = tokio::time::sleep(wait) => {}
        }
    }
}

/// Starts the planner for the proposals whose debounce is over; returns how
/// long until the next waiting one is due.
pub async fn start_due(state: &AppState) -> Result<Option<Duration>, sqlx::Error> {
    loop {
        let claimed = {
            let mut db = state.db.acquire().await?;
            repo::replan::claim_due(&mut db, CLAIM_BATCH).await?
        };
        for proposal in &claimed {
            start(state, proposal).await?;
        }
        if (claimed.len() as i64) < CLAIM_BATCH {
            break;
        }
    }
    repo::replan::next_due(&state.db).await
}

/// What starting a proposal came to.
#[derive(Debug)]
enum Drafted {
    /// The planner is drafting `plan_id` on `graph_id`.
    Started { graph_id: Uuid, plan_id: Uuid },
    /// Another plan is being drafted on the graph: wait.
    Busy { graph_id: Uuid },
    /// No plan can be drafted, for `reason`.
    Skipped {
        graph_id: Option<Uuid>,
        reason: String,
    },
    /// A failure that may pass (the database, a provider): try again later.
    Retry { graph_id: Option<Uuid> },
}

async fn start(state: &AppState, p: &Claimed) -> Result<(), sqlx::Error> {
    let events = &state.metrics.replan_proposals;
    match draft(state, p).await {
        Drafted::Started { graph_id, plan_id } => {
            repo::replan::started(&state.db, p.id, graph_id, plan_id).await?;
            events.add(&[("event", "started")], 1);
            tracing::info!(proposal = %p.id, root = %p.root_id, %graph_id, %plan_id,
                "a re-plan is being drafted; it waits on the graph to be applied");
        }
        Drafted::Busy { graph_id } => {
            let wait = state.settings.replan_debounce.max(BUSY_WAIT);
            repo::replan::postpone(&state.db, p.id, Some(graph_id), wait).await?;
            events.add(&[("event", "postponed")], 1);
        }
        Drafted::Retry { graph_id } => {
            repo::replan::postpone(&state.db, p.id, graph_id, SWEEP).await?;
            events.add(&[("event", "postponed")], 1);
        }
        Drafted::Skipped { graph_id, reason } => {
            repo::replan::skipped(&state.db, p.id, graph_id, &reason).await?;
            events.add(&[("event", "skipped")], 1);
            tracing::info!(proposal = %p.id, root = %p.root_id, reason, "a re-plan was not drafted");
        }
    }
    Ok(())
}

/// What a failure of `planner::start` or of giving the issue a graph means here.
fn drafted_after(err: AppError, graph_id: Option<Uuid>) -> Drafted {
    match err {
        AppError::Internal(_) | AppError::Unavailable(_) | AppError::RateLimited { .. } => {
            tracing::warn!(error = %error_chain(&err), "a re-plan could not be started; trying again later");
            Drafted::Retry { graph_id }
        }
        AppError::NotFound(what) => Drafted::Skipped {
            graph_id,
            reason: format!(
                "the person who made the change cannot work on this {what}: only a member of the \
                 issue's team can have it re-planned"
            ),
        },
        other => Drafted::Skipped {
            graph_id,
            reason: other.to_string(),
        },
    }
}

async fn draft(state: &AppState, p: &Claimed) -> Drafted {
    let skipped = |graph_id: Option<Uuid>, reason: &str| Drafted::Skipped {
        graph_id,
        reason: reason.to_owned(),
    };
    let Some(owner) = p.owner_id else {
        return skipped(None, "the person who made the change is gone");
    };
    if p.root_kind == ImpactEntityKind::Project {
        return skipped(None, PROJECT_HAS_NO_GRAPH);
    }
    let issue = match repo::issues::find(&state.db, owner, p.root_id).await {
        Ok(Some(issue)) => issue,
        Ok(None) => {
            return skipped(
                None,
                "the issue is gone, or the person who made the change can no longer see it",
            );
        }
        Err(err) => return drafted_after(err.into(), None),
    };
    let graph_id = match issue.graph_id {
        Some(id) => id,
        None => match issue_agents::create_graph(state, owner, &issue).await {
            Ok(id) => id,
            Err(err) => return drafted_after(err, None),
        },
    };
    match repo::replan::plan_streaming(&state.db, graph_id).await {
        Ok(true) => return Drafted::Busy { graph_id },
        Ok(false) => {}
        Err(err) => return drafted_after(err.into(), Some(graph_id)),
    }
    let instructions = match instructions(state, &issue, p).await {
        Ok(Some(text)) => text,
        Ok(None) => return skipped(Some(graph_id), "the assessment it answers is gone"),
        Err(err) => return drafted_after(err.into(), Some(graph_id)),
    };
    match planner::start(state, owner, graph_id, instructions).await {
        Ok(plan) => Drafted::Started {
            graph_id,
            plan_id: plan.id,
        },
        Err(err) => drafted_after(err, Some(graph_id)),
    }
}

/// The planner's instructions for proposal `p` of `issue`: its assessment as
/// every member of the issue's team may read it (the graph is that team's).
async fn instructions(
    state: &AppState,
    issue: &crate::domain::issue::Issue,
    p: &Claimed,
) -> Result<Option<String>, sqlx::Error> {
    let Some(assessment) = repo::impact::assessment(&state.db, p.assessment_id).await? else {
        return Ok(None);
    };
    let mut ids: Vec<Uuid> = assessment
        .impacts
        .iter()
        .map(|i| i.entity_id)
        .chain(
            assessment
                .violations
                .iter()
                .flat_map(|v| v.entities.iter().copied()),
        )
        .chain([assessment.entity_id, p.root_id])
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let named = repo::replan::named(&state.db, &ids).await?;
    // What every member of the issue's team sees: its own items and the projects of no team or
    // of that team. Other teams' items are left out, private or not.
    let readable = |n: &repo::replan::Named| {
        n.team_id == Some(issue.team_id)
            || (n.kind == ImpactEntityKind::Project && n.team_id.is_none())
    };
    let names: HashMap<Uuid, (String, String)> = named
        .iter()
        .filter(|n| readable(n))
        .map(|n| (n.id, (n.label.clone(), n.state.clone())))
        .collect();
    let hidden = assessment
        .impacts
        .iter()
        .filter(|i| !i.changed && !names.contains_key(&i.entity_id))
        .count();
    let assessment = assessment.redacted(&|id| names.contains_key(&id));
    let root = format!("{} \"{}\"", issue.identifier, issue.title);
    let root_text = if issue.description.trim().is_empty() {
        issue.title.clone()
    } else {
        format!("{}\n\n{}", issue.title, issue.description.trim())
    };
    let text = replan_instructions(&ReplanSeed {
        assessment: &assessment,
        root: &root,
        root_text: &root_text,
        names: &names,
        hidden,
    });
    debug_assert!(text.chars().count() <= REPLAN_INSTRUCTIONS_MAX);
    Ok(Some(text))
}
