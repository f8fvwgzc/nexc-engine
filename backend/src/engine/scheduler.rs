//! The run scheduler.
//!
//! A run is created `queued`; a dispatcher on some backend instance claims it
//! (`FOR UPDATE SKIP LOCKED`) and executes its DAG: ready nodes are taken
//! from a priority queue ordered by critical-path length, at most
//! `max_concurrency` run at once (semaphore), each attempt has a timeout,
//! retryable failures back off with full jitter, descendants of failed nodes
//! are skipped, and unchanged nodes are served from the content-hash cache.
//!
//! The instance executing a run holds a lease on it (docs/HA.md), renewed
//! every third of `NEXC_RUN_LEASE_SECS` ([`keep_leases`]). A run whose lease
//! ran out is taken over by another instance's dispatcher and resumed from
//! what was written: succeeded nodes keep their outputs, a node that was
//! running is queued again with its next attempt and an `interrupted` span,
//! a node that waited for an approval waits again here, and the trace
//! continues its chain. Every write of the executor is fenced by the lease
//! ([`lease::begin`]): once a run changed hands, its former holder writes
//! nothing more for it.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::analysis::dependency_graph;
use super::approvals::{self, Reattached};
use super::defend::{About, Finding};
use super::executor::{self, ExecContext, ExecError, ExecOutput};
use super::lease::{self, Lease, WriteError};
use super::policy as policies;
use super::trace::{self, Sealed, Tail, TraceHandle, TraceSink};
use super::{artifacts, credentials, editor, guardrails, knowledge, usage};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::agent::{Agent, AgentStatus};
use crate::domain::alert::AlertKind;
use crate::domain::approval::Approval;
use crate::domain::audit::AuditAction;
use crate::domain::capabilities::{Capabilities, DataLabel};
use crate::domain::context;
use crate::domain::evidence::{self, Given};
use crate::domain::graph::{Executor, GraphEdge, GraphNode, NodeStatus};
use crate::domain::guardrails::Guardrails;
use crate::domain::injection::{self, InjectionFinding};
use crate::domain::ontology::Ontology;
use crate::domain::policy::{self, Action, Decision};
use crate::domain::policy_document::Policy;
use crate::domain::policy_document::{EvidenceAction, InjectionAction, Severity};
use crate::domain::prompt::{AGENT_UPSTREAM_CHARS, UPSTREAM_CHARS, UpstreamOutput};
use crate::domain::recording::Redaction;
use crate::domain::run::{Run, RunStatus, final_status};
use crate::domain::seal::SignedSeal;
use crate::domain::settings::{ConfigScope, LlmProviderKind};
use crate::domain::trace::{SYSTEM_PRINCIPAL, SpanDraft, SpanKind, SpanStatus};
use crate::domain::usage::{UsageEvent, UsagePurpose};
use crate::domain::workspace::WorkspaceRole;
use crate::dsa::priority::ReadyQueue;
use crate::llm::LlmTarget;
use crate::llm::pricing::{cost_usd, cost_usd_cached};
use crate::llm::retry::full_jitter;
use crate::ontology::vocabulary::{self, Actor, VocabularyError};
use crate::realtime::events::{LogLevel, NodeStatusEvent, SseEvent};
use crate::repo::audit::Subject;
use crate::repo::run_leases;
use crate::repo::runs::{NodeOutcome, RunRow, StoredNode};
use crate::repo::{self, OrNotFound};
use crate::security::agent_token::OnBehalfOf;
use crate::{kernel, memory, planner};

/// How often the dispatcher looks for queued runs and for leases that ran out.
const DISPATCH_POLL: Duration = Duration::from_secs(2);
const CANCEL_POLL: Duration = Duration::from_secs(1);
/// The decision of the `policy` span on a node whose attempt a takeover interrupted.
pub const INTERRUPTED_DECISION: &str = "interrupted";
/// The decision of the `policy` span on a node that waits for an approval again after a takeover.
pub const REATTACHED_DECISION: &str = "reattached";
/// The rule of both: the run's lease.
pub const LEASE_RULE: &str = "lease";

/// How long a lease is trusted here without the database confirming it:
/// two thirds of it, counted from when the last confirmed request was sent.
/// The database lets it run out a whole lease after that, so a holder cut
/// off from the database stops its runs a third of a lease before any peer
/// can take them over.
fn trusted_for(lease: Duration) -> Duration {
    lease * 2 / 3
}

/// Options of `POST /graphs/{gid}/runs`.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub node_ids: Option<Vec<Uuid>>,
    pub max_concurrency: Option<u32>,
    pub force: bool,
}

/// Queues a run of the selected nodes (all by default) and wakes the dispatcher.
pub async fn create_run(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
    opts: RunOptions,
) -> Result<Run, AppError> {
    // The vocabulary is not needed here: a run can be queued while onto-intelli is away, and
    // its nodes ask for it when they run.
    let (graph, nodes, _) = editor::load_structure(state, owner, graph_id).await?;
    if nodes.is_empty() {
        return Err(AppError::Unprocessable(
            "the graph has no nodes to run".into(),
        ));
    }
    let selected: Vec<&GraphNode> = match &opts.node_ids {
        None => nodes.iter().collect(),
        Some(ids) => {
            let wanted: HashSet<Uuid> = ids.iter().copied().collect();
            let found: Vec<&GraphNode> = nodes.iter().filter(|n| wanted.contains(&n.id)).collect();
            if found.len() != wanted.len() {
                return Err(AppError::field(
                    "node_ids",
                    "contains ids that are not nodes of this graph",
                ));
            }
            found
        }
    };
    if selected.iter().any(|n| n.executor != Executor::Symphony) {
        let llm = credentials::require(state, owner, graph.workspace_id).await?;
        guardrails::admit(state, graph.workspace_id, owner, llm.target.provider).await?;
    }
    let concurrency = opts
        .max_concurrency
        .unwrap_or(state.settings.max_concurrency as u32) as i32;
    let nodes: Vec<(Uuid, Executor)> = selected.iter().map(|n| (n.id, n.executor)).collect();
    let mut tx = state.db.begin().await?;
    let run_id =
        repo::runs::create(&mut tx, graph_id, owner, concurrency, opts.force, &nodes).await?;
    tx.commit().await?;
    state.engine.wake();
    load_run(state, run_id).await
}

/// Loads a run with its node results (no ownership check).
pub async fn load_run(state: &AppState, run_id: Uuid) -> Result<Run, AppError> {
    let row = repo::runs::find_row(&state.db, run_id)
        .await
        .or_not_found("run")?;
    let node_runs = repo::runs::node_runs(&state.db, run_id).await?;
    Ok(row.into_run(node_runs))
}

/// Requests cancellation of a run owned by `owner` and returns its state.
pub async fn cancel_run(state: &AppState, owner: Uuid, run_id: Uuid) -> Result<Run, AppError> {
    let row = repo::runs::request_cancel(&state.db, owner, run_id)
        .await
        .or_not_found("run")?;
    if row.status == RunStatus::Cancelled {
        for node_id in
            repo::runs::close_unfinished_nodes(&state.db, run_id, NodeStatus::Cancelled).await?
        {
            repo::nodes::set_status(&state.db, node_id, NodeStatus::Cancelled, None).await?;
        }
        let run = load_run(state, run_id).await?;
        state
            .hub
            .publish(run.graph_id, SseEvent::RunFinished { run: run.clone() });
        return Ok(run);
    }
    if state.engine.cancel_local(run_id) {
        wait_until_terminal(state, run_id, Duration::from_secs(5)).await;
    }
    load_run(state, run_id).await
}

async fn wait_until_terminal(state: &AppState, run_id: Uuid, limit: Duration) {
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        match repo::runs::find_row(&state.db, run_id).await {
            Ok(Some(row)) if !row.status.is_terminal() => {
                tokio::time::sleep(Duration::from_millis(100)).await
            }
            _ => return,
        }
    }
}

/// Claims and executes queued runs, and takes over runs whose lease ran out,
/// until `shutdown`. A draining instance does neither.
pub async fn dispatcher(state: AppState, shutdown: CancellationToken) {
    loop {
        claim_queued(&state).await;
        take_over_expired(&state).await;
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = state.engine.woken() => {}
            _ = tokio::time::sleep(DISPATCH_POLL) => {}
        }
    }
}

/// The lease, in seconds, as the run rows take it.
fn lease_secs(state: &AppState) -> f64 {
    state.settings.run_lease.as_secs_f64()
}

/// Claims every queued run and executes each here.
async fn claim_queued(state: &AppState) {
    while !state.engine.is_draining() {
        let asked = Instant::now();
        match repo::runs::claim_next(&state.db, state.engine.instance(), lease_secs(state)).await {
            Ok(Some(row)) => {
                tokio::spawn(execute(
                    state.clone(),
                    row,
                    Start::Fresh {
                        opening: Vec::new(),
                        refusal: None,
                    },
                    asked,
                ));
            }
            Ok(None) => return,
            Err(err) => {
                tracing::error!(error = %err, "cannot claim runs");
                return;
            }
        }
    }
}

/// Takes over every run whose lease ran out and resumes each here.
async fn take_over_expired(state: &AppState) {
    while !state.engine.is_draining() {
        let local = state.engine.local_runs();
        let asked = Instant::now();
        match repo::runs::take_over(
            &state.db,
            state.engine.instance(),
            lease_secs(state),
            &local,
        )
        .await
        {
            Ok(Some(taken)) => {
                tracing::warn!(run_id = %taken.row.id, previous = ?taken.previous,
                    epoch = taken.row.lease_epoch, released = taken.released,
                    "run taken over from an instance that no longer holds it");
                if let Some(theirs) = taken
                    .previous_lease_secs
                    .filter(|theirs| (theirs - lease_secs(state)).abs() >= 0.5)
                {
                    tracing::warn!(run_id = %taken.row.id, theirs, ours = lease_secs(state),
                        "replicas run with different NEXC_RUN_LEASE_SECS; set the same on every replica");
                }
                tokio::spawn(execute(
                    state.clone(),
                    taken.row,
                    Start::Resume {
                        previous: taken.previous,
                        released: taken.released,
                    },
                    asked,
                ));
            }
            Ok(None) => return,
            Err(err) => {
                tracing::error!(error = %err, "cannot take over runs");
                return;
            }
        }
    }
}

/// Hands every run executing here to a peer (the instance shuts down): it
/// stops claiming and fails readiness, each run stops at once, writes its
/// trace up to here and releases its lease, so a peer's dispatcher takes it
/// over at its next look instead of when the lease would have run out.
/// Waits for that at most `limit`; a run still here then is abandoned, and
/// taken over when its lease runs out. Returns how many runs were handed off.
pub async fn hand_off(state: &AppState, limit: Duration) -> usize {
    state.engine.start_draining();
    let runs = state.engine.hand_off_all();
    if runs > 0 {
        tracing::info!(runs, "draining: handing runs to peers");
    }
    let deadline = Instant::now() + limit;
    while state.engine.active_count() > 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let left = state.engine.abandon_all();
    if left > 0 {
        tracing::warn!(
            left,
            "runs not handed off in time; peers take them over when their lease runs out"
        );
    }
    runs
}

/// Renews the leases of the runs executing here every third of the lease,
/// until `shutdown`.
pub async fn keep_leases(state: AppState, shutdown: CancellationToken) {
    let every = (state.settings.run_lease / 3).max(Duration::from_millis(100));
    let mut ticker = tokio::time::interval(every);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = ticker.tick() => renew_leases(&state, every).await,
        }
    }
}

/// Renews every lease held here in one statement (given at most `limit`, a
/// third of the lease). A lease the database did not renew belongs to a run
/// that changed hands: it is lost. So is one the database has not confirmed
/// for two thirds of a lease ([`trusted_for`]; this instance is cut off from
/// it): the run stops here, before a peer can take it over, and whoever
/// takes it over resumes it.
async fn renew_leases(state: &AppState, limit: Duration) {
    let held = state.engine.held_leases();
    if held.is_empty() {
        return;
    }
    let pairs: Vec<(Uuid, i64)> = held
        .iter()
        .filter_map(|l| l.epoch().map(|epoch| (l.run_id(), epoch)))
        .collect();
    let sent = Instant::now();
    let renewed = tokio::time::timeout(
        limit,
        repo::runs::renew(
            &state.lease_db,
            state.engine.instance(),
            &pairs,
            lease_secs(state),
        ),
    )
    .await;
    match renewed {
        Ok(Ok(renewed)) => {
            let renewed: HashSet<Uuid> = renewed.into_iter().collect();
            for lease in &held {
                if renewed.contains(&lease.run_id()) {
                    lease.confirmed(sent);
                } else {
                    not_renewed(state, lease).await;
                }
            }
        }
        failed => {
            match failed {
                Ok(Err(err)) => tracing::warn!(error = %err, "run leases not renewed"),
                _ => tracing::warn!("run leases not renewed: the database did not answer in time"),
            }
            let trusted = trusted_for(state.settings.run_lease);
            for lease in held.iter().filter(|l| l.unconfirmed_for() >= trusted) {
                give_up(
                    state,
                    lease,
                    "the database did not confirm the lease for two thirds of a lease",
                )
                .await;
            }
        }
    }
}

/// A lease the renewal left out: the run finished (nothing to do) or it
/// went on elsewhere (the lease is lost).
async fn not_renewed(state: &AppState, lease: &Lease) {
    match repo::runs::find_row(&state.db, lease.run_id()).await {
        Ok(Some(row)) if row.status == RunStatus::Running => {
            let holder = row
                .claimed_by
                .map_or_else(|| "nobody".to_owned(), |id| id.to_string());
            let why = format!("the run is held by {holder} at epoch {}", row.lease_epoch);
            give_up(state, lease, &why).await;
        }
        Ok(_) => {}
        Err(err) => {
            tracing::warn!(run_id = %lease.run_id(), error = %err, "lease not renewed; trying again")
        }
    }
}

/// Gives a lease up: the run stops here and nothing more is written for it.
async fn give_up(state: &AppState, lease: &Lease, why: &str) {
    if lease.is_lost() {
        return;
    }
    let epoch = lease.epoch().unwrap_or_default();
    tracing::warn!(run_id = %lease.run_id(), epoch, why, "run lease lost; the run stops here");
    lease.lose();
    lease::record(state, lease, run_leases::LOST, epoch, None, why).await;
}

/// How a run comes to execute here.
enum Start {
    /// Claimed queued: `opening` spans are chained before anything else and
    /// `refusal`, when set, fails every node at once.
    Fresh {
        opening: Vec<SpanDraft>,
        refusal: Option<String>,
    },
    /// Taken over from `previous`, whose lease ran out (or who `released` it).
    Resume {
        previous: Option<Uuid>,
        released: bool,
    },
}

/// Executes a run this instance claimed (`repo::runs::claim`): the A2A
/// adapter runs an inbound task this way, with `opening` spans chained
/// before anything else (who called, the decision that admitted it) and,
/// when the firewall refused the task, `refusal` failing every node at once
/// so the run is on the record without the agent being started.
pub async fn execute_claimed(
    state: AppState,
    row: RunRow,
    opening: Vec<SpanDraft>,
    refusal: Option<String>,
    asked: Instant,
) {
    execute(state, row, Start::Fresh { opening, refusal }, asked).await;
}

/// Keeps a run registered here while its execution lives. Dropping it (the
/// run finished or stopped here, or its task panicked) forgets the run, so
/// its lease is no longer renewed and runs out for a peer to take it over.
struct Registered {
    engine: Arc<super::Engine>,
    lease: Lease,
}

impl Drop for Registered {
    fn drop(&mut self) {
        self.engine.unregister_run(&self.lease);
    }
}

/// Executes a run under the lease its row carries (claimed by a statement
/// sent at `asked`) until it finishes, or until the lease says to stop (it
/// was lost, or this instance hands it to a peer).
async fn execute(state: AppState, row: RunRow, start: Start, asked: Instant) {
    let lease = Lease::held_since(row.id, row.lease_epoch, state.engine.instance(), asked);
    let token = state.engine.register_run(lease.clone());
    let _registered = Registered {
        engine: state.engine.clone(),
        lease: lease.clone(),
    };
    // Claimed just as the instance began to shut down: handed on at once.
    if state.engine.is_draining() {
        lease.hand_off();
    }
    let started = Instant::now();
    let tail = match &start {
        Start::Fresh { .. } => {
            lease::record(
                &state,
                &lease,
                run_leases::CLAIMED,
                row.lease_epoch,
                None,
                "",
            )
            .await;
            if let Ok(run) = load_run(&state, row.id).await {
                state
                    .hub
                    .publish(row.graph_id, SseEvent::RunStarted { run });
            }
            Tail::GENESIS
        }
        Start::Resume { previous, released } => {
            let how = if *released {
                "released by its holder, which shut down"
            } else {
                "its holder stopped renewing the lease"
            };
            lease::record(
                &state,
                &lease,
                run_leases::TAKEN_OVER,
                row.lease_epoch,
                *previous,
                how,
            )
            .await;
            match Tail::of_run(&state, row.id).await {
                Ok(tail) => tail,
                Err(err) => {
                    // Without the end of the chain nothing can be added to it: the lease runs
                    // out again and the next takeover tries again.
                    tracing::error!(run_id = %row.id, error = %err, "the run's trace could not be read; not resumed");
                    lease.lose();
                    return;
                }
            }
        }
    };
    // Every span of the run goes through one chain, in order of receipt.
    let sink = trace::start(state.clone(), row.id, row.graph_id, lease.clone(), tail);
    let driving = work(&state, &row, &lease, token.clone(), sink.handle(), start);
    let statuses = tokio::select! {
        biased;
        _ = lease.stopped() => None,
        statuses = driving => Some(statuses),
    };
    let Some(statuses) = statuses.filter(|_| !lease.is_stopped()) else {
        stop_here(&state, &row, &lease, sink).await;
        return;
    };
    let cancelled = token.is_cancelled();
    let status = if statuses.is_empty() {
        RunStatus::Failed
    } else {
        final_status(&statuses, cancelled)
    };
    let sealed = sink.seal().await;
    match finish_run(&state, &lease, row.id, status, cancelled, sealed).await {
        Ok(()) => {
            state.metrics.runs.add(&[("status", status.as_str())], 1);
            state.metrics.run_duration.observe(started.elapsed());
        }
        // Another instance holds the run now; it finishes it.
        Err(WriteError::Fenced) => {}
        // The lease runs out, and whoever takes the run over finishes it from what was written.
        Err(err) => tracing::error!(run_id = %row.id, error = %err, "cannot finish run"),
    }
}

/// Prepares and drives the run: from the start, or from what was written
/// when it was taken over. Returns the final status of every node.
async fn work(
    state: &AppState,
    row: &RunRow,
    lease: &Lease,
    token: CancellationToken,
    trace: TraceHandle,
    start: Start,
) -> Vec<NodeStatus> {
    let resume = match start {
        Start::Fresh { opening, refusal } => {
            for span in opening {
                trace.emit(span);
            }
            if let Some(reason) = refusal {
                fail_all_nodes(state, lease, row, &reason).await;
                return vec![NodeStatus::Failed];
            }
            None
        }
        Start::Resume { previous, released } => {
            // A cancel asked for while nobody held the run stops it here before any node is
            // queued again: nothing more is sent to the runtime or the provider for it.
            if row.cancel_requested {
                token.cancel();
            }
            // Commands the former holder never took are this instance's now, before any node
            // starts again: a cancel of a sub-agent waits for the attempt that replaces its own.
            super::commands::deliver(state, row.id).await;
            // The holder shut down and handed the run over: the attempts it cut short are run
            // again without counting against the nodes' attempts.
            if released && let Err(err) = count_handoffs(state, lease, row.id).await {
                tracing::error!(run_id = %row.id, error = %err, "hand-off not counted; not resumed");
                lease.lose();
                return Vec::new();
            }
            match repo::runs::stored_nodes(&state.db, row.id).await {
                Ok(stored) => Some(Resume {
                    stored: stored.into_iter().map(|n| (n.node_id, n)).collect(),
                    previous,
                    released,
                }),
                Err(err) => {
                    tracing::error!(run_id = %row.id, error = %err, "the run's nodes could not be read; not resumed");
                    lease.lose();
                    return Vec::new();
                }
            }
        }
    };
    let resumed = resume.is_some();
    // A graph deleted after the run was queued, or while it changed hands: the run stops as
    // its deletion asked, cancelled rather than failed.
    if repo::graphs::is_deleted(&state.db, row.graph_id)
        .await
        .unwrap_or(false)
    {
        token.cancel();
        return vec![NodeStatus::Cancelled];
    }
    match Execution::prepare(state, row, lease.clone(), token, trace, resumed).await {
        Ok(execution) => execution.drive(resume).await,
        Err(err) => {
            tracing::error!(run_id = %row.id, error = %err, "run preparation failed");
            Vec::new()
        }
    }
}

/// Counts the hand-off of a released run against the nodes it cut short, under the lease.
async fn count_handoffs(state: &AppState, lease: &Lease, run_id: Uuid) -> Result<(), WriteError> {
    let mut tx = lease::begin(state, lease, "hand-off count").await?;
    repo::runs::count_handoffs(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(())
}

/// The run stops here without finishing. When this instance hands it to a
/// peer (it shuts down), every span chained so far is written under the
/// lease still held and the lease is released, so the peer continues the
/// chain at once. When the lease was lost, nothing more is written.
async fn stop_here(state: &AppState, row: &RunRow, lease: &Lease, sink: TraceSink) {
    let epoch = lease.epoch().unwrap_or_default();
    if !lease.is_handing_off() || lease.is_lost() {
        drop(sink);
        tracing::warn!(run_id = %row.id, epoch, "run stopped here: this instance no longer holds its lease");
        return;
    }
    let sealed = sink.seal().await;
    match repo::runs::release(&state.db, row.id, epoch).await {
        Ok(Some(next)) => {
            let detail = format!(
                "handed off on shutdown with {} span(s) written; the next holder takes epoch {}",
                sealed.spans,
                next + 1
            );
            lease::record(state, lease, run_leases::RELEASED, epoch, None, &detail).await;
            tracing::info!(run_id = %row.id, epoch, "run handed off to a peer");
        }
        Ok(None) => {
            tracing::info!(run_id = %row.id, epoch, "run not released: its lease had moved on")
        }
        Err(err) => {
            tracing::error!(run_id = %row.id, error = %err, "lease not released; a peer takes the run over when it runs out")
        }
    }
    lease.lose();
}

/// Fails every node of a run before any is started, with `reason` (a task
/// the firewall refused).
async fn fail_all_nodes(state: &AppState, lease: &Lease, row: &RunRow, reason: &str) {
    let closed = async {
        let mut tx = lease::begin(state, lease, "refused run's nodes").await?;
        let nodes =
            repo::runs::close_unfinished_nodes(&mut *tx, row.id, NodeStatus::Failed).await?;
        for &node_id in &nodes {
            repo::runs::set_node_error(&mut *tx, row.id, node_id, reason).await?;
            repo::nodes::set_status(&mut *tx, node_id, NodeStatus::Failed, None).await?;
        }
        tx.commit().await?;
        Ok::<Vec<Uuid>, WriteError>(nodes)
    }
    .await;
    let nodes = match closed {
        Ok(nodes) => nodes,
        Err(err) => {
            tracing::error!(run_id = %row.id, error = %err, "refused run's nodes not closed");
            return;
        }
    };
    for node_id in nodes {
        state.hub.publish(
            row.graph_id,
            SseEvent::NodeStatus(NodeStatusEvent {
                run_id: row.id,
                node_id,
                status: NodeStatus::Failed,
                attempt: 0,
                error: Some(reason.to_owned()),
                cached: false,
            }),
        );
    }
}

/// Closes the run under its lease: leftover nodes, the final status, the
/// seal of its trace (on the run and in the audit log), and `run.finished`.
async fn finish_run(
    state: &AppState,
    lease: &Lease,
    run_id: Uuid,
    status: RunStatus,
    cancelled: bool,
    sealed: Sealed,
) -> Result<(), WriteError> {
    let leftover = if cancelled {
        NodeStatus::Cancelled
    } else {
        NodeStatus::Skipped
    };
    let hash = sealed.hash;
    // The seal is signed as the run ends (docs/AGENTSECOPS.md §12).
    let seal =
        hash.map(|h| SignedSeal::sign(state.signing_keys.current(), run_id, sealed.spans, &h));
    let mut tx = lease::begin_run_update(state, lease, "run status").await?;
    for node_id in repo::runs::close_unfinished_nodes(&mut *tx, run_id, leftover).await? {
        repo::nodes::set_status(&mut *tx, node_id, leftover, None).await?;
    }
    let row = repo::runs::finish(
        &mut *tx,
        run_id,
        status,
        hash.as_ref().map(|h| h.as_slice()),
        seal.as_ref(),
    )
    .await?;
    // The issues the graph executes get the result once, under the run's fence.
    let issues = super::issue_report::report(&mut tx, &row).await?;
    // The seal is on record in the transaction that makes it: evidence bundles require it.
    if let Some(hash) = hash {
        seal_audit(&mut tx, &row, hash, sealed.spans).await?;
    }
    tx.commit().await?;
    if !issues.is_empty() {
        tracing::info!(%run_id, issues = issues.len(), "run reported to its issues");
    }
    // Every agent that worked is scored against what it usually does, and learns the run.
    let assessed = super::risk::assess(state, &row).await;
    if assessed.alerts > 0 || assessed.quarantined > 0 {
        tracing::info!(%run_id, agents = assessed.agents, alerts = assessed.alerts,
            quarantined = assessed.quarantined, "run assessed");
    }
    let graph_id = row.graph_id;
    let run = row.into_run(repo::runs::node_runs(&state.db, run_id).await?);
    state.hub.publish(graph_id, SseEvent::RunFinished { run });
    Ok(())
}

/// Writes the final hash of a run's trace chain, and the key its seal is
/// signed with, to its workspace's audit log (`trace_sealed`, subject: the
/// run id), in the transaction that seals the run. A graph outside any
/// workspace has no audit log; its seal lives on the run alone.
pub async fn seal_audit(
    conn: &mut sqlx::PgConnection,
    row: &RunRow,
    hash: [u8; 32],
    spans: i64,
) -> Result<(), sqlx::Error> {
    let Some(workspace) = repo::graphs::workspace_of(&mut *conn, row.graph_id).await? else {
        return Ok(());
    };
    let mut detail = format!("{spans} span(s), sha256 {}", hex::encode(hash));
    if let Some(seal) = &row.seal {
        detail.push_str(&format!(", signed by {}", seal.kid));
    }
    repo::audit::record(
        &mut *conn,
        workspace,
        row.owner_id,
        AuditAction::TraceSealed,
        Subject::Text(&row.id.to_string()),
        &detail,
    )
    .await
}

/// Shared, immutable data of one run execution.
struct RunData {
    run_id: Uuid,
    graph_id: Uuid,
    owner: Uuid,
    /// The owner's role in the graph's workspace as the run was prepared (the
    /// `on_behalf_of` of every agent token the run issues).
    owner_role: Option<WorkspaceRole>,
    goal: String,
    force: bool,
    target: LlmTarget,
    /// Whose configuration `target` came from; usage is booked to it.
    credential: ConfigScope,
    /// The workspace's guardrails, as they stood when the run started.
    policy: Guardrails,
    /// The latest version of every policy of the workspace, as the run started.
    policies: Arc<Vec<Policy>>,
    workspace_id: Option<Uuid>,
    graph_name: String,
    /// The graph's vocabulary in onto-intelli as the run started; `None` when
    /// onto-intelli could not be reached and nothing was kept, in which case
    /// every node asks again before it runs ([`node_vocabulary`]).
    ontology: Option<Arc<Ontology>>,
    edges: Vec<GraphEdge>,
    /// The run's trace chain.
    trace: TraceHandle,
    /// The agent every node runs as, when the graph is an exposed agent's
    /// A2A graph (its tasks are its own, whatever role the node names).
    pinned_agent: Option<Uuid>,
    /// The lease the run executes under: every write is fenced on it.
    lease: Lease,
}

impl RunData {
    /// A ledger entry for a call made on behalf of this run.
    fn usage_event(
        &self,
        purpose: UsagePurpose,
        model: &str,
        tokens_in: i64,
        tokens_out: i64,
        cost_usd: f64,
        context_chars_saved: i64,
    ) -> UsageEvent {
        UsageEvent {
            workspace_id: self.workspace_id,
            user_id: self.owner,
            graph_id: Some(self.graph_id),
            run_id: Some(self.run_id),
            purpose,
            provider: self.target.provider,
            model: model.to_owned(),
            credential: self.credential,
            tokens_in,
            tokens_out,
            cost_usd,
            context_chars_saved,
            agent_id: None,
            node_id: None,
            span_id: None,
        }
    }
}

/// The mutable state of one run while it executes.
struct Execution {
    state: AppState,
    data: Arc<RunData>,
    token: CancellationToken,
    nodes: Vec<GraphNode>,
    max_concurrency: usize,
}

/// Result of one node task.
struct NodeResult {
    index: usize,
    status: NodeStatus,
    output: Option<String>,
}

/// What a run that was taken over had written when its former holder stopped.
struct Resume {
    /// Every node result as stored, by node.
    stored: HashMap<Uuid, StoredNode>,
    /// The instance that held the run before.
    previous: Option<Uuid>,
    /// Whether it released the run (it shut down) rather than stopped renewing.
    released: bool,
}

/// How a node of a resumed run is picked up. `budget` is how many attempts
/// it may make: `NEXC_MAX_ATTEMPTS` plus those cut short by a hand-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pickup {
    /// From its first attempt (a fresh run, or a node that had not started).
    Fresh,
    /// It was running, or was queued again after that: the attempt after
    /// `attempt` runs.
    Interrupted { attempt: i32, budget: i32 },
    /// It waited for an approval during `attempt`: it waits here for the decision.
    Waiting { attempt: i32, budget: i32 },
}

impl Execution {
    async fn prepare(
        state: &AppState,
        row: &RunRow,
        lease: Lease,
        token: CancellationToken,
        trace: TraceHandle,
        resumed: bool,
    ) -> anyhow::Result<Self> {
        let graph = repo::graphs::find(&state.db, row.owner_id, row.graph_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("graph of run {} no longer exists", row.id))?;
        let all_nodes = repo::nodes::list(&state.db, row.graph_id).await?;
        let edges = repo::edges::list(&state.db, row.graph_id).await?;
        let selected: HashSet<Uuid> = repo::runs::node_runs(&state.db, row.id)
            .await?
            .iter()
            .map(|n| n.node_id)
            .collect();
        let nodes: Vec<GraphNode> = all_nodes
            .into_iter()
            .filter(|n| selected.contains(&n.id))
            .collect();
        let resolved = credentials::resolve(state, row.owner_id, graph.workspace_id).await?;
        let (target, credential) = (resolved.target, resolved.scope);
        let policies = match graph.workspace_id {
            Some(workspace) => state.caches.policies(&state.db, workspace).await?,
            None => Arc::new(Vec::new()),
        };
        // The workspace level governs the whole run; each node records every level. A resumed
        // run keeps the versions it started under; its nodes record the ones they run under.
        if !resumed {
            let versions = policies::versions_of(&policies::levels_for(&policies, None, None));
            let mut tx = lease::begin_run_update(state, &lease, "run policy versions").await?;
            repo::runs::set_policy_versions(&mut *tx, row.id, &versions).await?;
            tx.commit().await?;
        }
        let owner_role = match graph.workspace_id {
            Some(workspace) => {
                repo::workspaces::role_of(&state.db, workspace, row.owner_id).await?
            }
            None => None,
        };
        let pinned_agent = repo::a2a::pinned_agent_of_graph(&state.db, row.graph_id).await?;
        let ontology = match vocabulary::of(state, graph.vocabulary_ref(), Actor::Runtime).await {
            Ok(vocabulary) => Some(vocabulary),
            Err(err) => {
                tracing::warn!(run_id = %row.id, error = %err, "the graph's vocabulary is not at hand; each node asks for it again");
                None
            }
        };
        let data = RunData {
            run_id: row.id,
            graph_id: row.graph_id,
            owner: row.owner_id,
            owner_role,
            goal: graph.goal,
            force: row.force,
            target,
            credential,
            policy: guardrails::load(state, graph.workspace_id).await?,
            policies,
            workspace_id: graph.workspace_id,
            graph_name: graph.name,
            ontology,
            edges,
            trace,
            pinned_agent,
            lease,
        };
        Ok(Execution {
            state: state.clone(),
            data: Arc::new(data),
            token,
            nodes,
            max_concurrency: row.max_concurrency.max(1) as usize,
        })
    }

    /// Executes the DAG, from the start or from what a former holder of the
    /// run wrote (`resume`); returns the final status of every node.
    async fn drive(self, resume: Option<Resume>) -> Vec<NodeStatus> {
        let (_, dag) = dependency_graph(&self.nodes, &self.data.edges);
        let priority = dag.remaining_depth();
        let mut statuses = vec![NodeStatus::Queued; dag.len()];
        let mut pickups = vec![Pickup::Fresh; dag.len()];
        let mut outputs = self.initial_outputs().await;
        let max_attempts = self.state.settings.max_attempts as i32;
        // Stopped with the run, however it ends (the future may be dropped mid-way).
        let _watcher = spawn_cancel_watcher(
            self.state.clone(),
            self.data.lease.clone(),
            self.token.clone(),
        )
        .drop_guard();
        let semaphore = Arc::new(Semaphore::new(self.max_concurrency));
        let mut tasks = JoinSet::new();
        match &resume {
            None => {
                for node in &self.nodes {
                    publish_status(
                        &self.state,
                        &self.data,
                        node.id,
                        NodeStatus::Queued,
                        0,
                        None,
                        false,
                    );
                }
            }
            Some(resume) => {
                for (v, node) in self.nodes.iter().enumerate() {
                    let Some(stored) = resume.stored.get(&node.id) else {
                        continue;
                    };
                    match stored.status {
                        NodeStatus::Succeeded => {
                            statuses[v] = NodeStatus::Succeeded;
                            if let Some(output) = &stored.output {
                                outputs.insert(node.id, output.clone());
                            }
                        }
                        NodeStatus::Failed | NodeStatus::Skipped | NodeStatus::Cancelled => {
                            statuses[v] = stored.status;
                        }
                        NodeStatus::Running => {
                            let budget = max_attempts + stored.handoffs;
                            pickups[v] = Pickup::Interrupted {
                                attempt: stored.attempt,
                                budget,
                            };
                            statuses[v] = self.interrupted(resume, stored, budget).await;
                        }
                        NodeStatus::WaitingApproval => {
                            pickups[v] = Pickup::Waiting {
                                attempt: stored.attempt,
                                budget: max_attempts + stored.handoffs,
                            };
                            statuses[v] = NodeStatus::WaitingApproval;
                            self.reattached(resume, stored);
                        }
                        // Queued again by a holder that stopped before it started the node: the
                        // attempt after the one interrupted runs, as that holder would have.
                        NodeStatus::Queued | NodeStatus::Idle if stored.attempt > 0 => {
                            pickups[v] = Pickup::Interrupted {
                                attempt: stored.attempt,
                                budget: max_attempts + stored.handoffs,
                            };
                        }
                        NodeStatus::Queued | NodeStatus::Idle => {}
                    }
                }
                // Descendants of what ended without success are skipped, as they would have been.
                for v in 0..dag.len() {
                    if matches!(
                        statuses[v],
                        NodeStatus::Failed | NodeStatus::Skipped | NodeStatus::Cancelled
                    ) {
                        for w in dag.descendants(v) {
                            if statuses[w] == NodeStatus::Queued {
                                statuses[w] = NodeStatus::Skipped;
                                skip_node(&self.state, &self.data, self.nodes[w].id).await;
                            }
                        }
                    }
                }
                // A node that waited for an approval waits here, for the decision on it.
                for v in 0..dag.len() {
                    if let (NodeStatus::WaitingApproval, Pickup::Waiting { attempt, budget }) =
                        (statuses[v], pickups[v])
                    {
                        tasks.spawn(resume_waiting(
                            self.state.clone(),
                            self.data.clone(),
                            self.token.clone(),
                            semaphore.clone(),
                            v,
                            self.nodes[v].clone(),
                            upstream_of(&self.nodes[v], &self.data.edges, &outputs),
                            attempt,
                            budget,
                        ));
                    }
                }
            }
        }
        // What a node still waits for: its upstream nodes that have not succeeded.
        let mut indegree: Vec<usize> = (0..dag.len())
            .map(|v| {
                dag.predecessors(v)
                    .iter()
                    .filter(|&&u| statuses[u] != NodeStatus::Succeeded)
                    .count()
            })
            .collect();
        let mut ready = ReadyQueue::default();
        for v in (0..dag.len()).filter(|&v| indegree[v] == 0 && statuses[v] == NodeStatus::Queued) {
            ready.push(priority[v], v);
        }
        loop {
            while !self.token.is_cancelled()
                && let Some(v) = ready.pop()
            {
                statuses[v] = NodeStatus::Running;
                let (first_attempt, budget) = match pickups[v] {
                    Pickup::Interrupted { attempt, budget } => (attempt + 1, budget),
                    _ => (1, max_attempts),
                };
                tasks.spawn(run_node(
                    self.state.clone(),
                    self.data.clone(),
                    self.token.clone(),
                    semaphore.clone(),
                    v,
                    self.nodes[v].clone(),
                    upstream_of(&self.nodes[v], &self.data.edges, &outputs),
                    first_attempt,
                    budget,
                    None,
                ));
            }
            let Some(joined) = tasks.join_next().await else {
                break;
            };
            let result = match joined {
                Ok(r) => r,
                Err(err) => {
                    tracing::error!(error = %err, "node task panicked");
                    continue;
                }
            };
            statuses[result.index] = result.status;
            if let (NodeStatus::Succeeded, Some(output)) = (result.status, result.output) {
                outputs.insert(self.nodes[result.index].id, output);
                for &w in dag.successors(result.index) {
                    indegree[w] -= 1;
                    if indegree[w] == 0 && statuses[w] == NodeStatus::Queued {
                        ready.push(priority[w], w);
                    }
                }
            } else if !self.token.is_cancelled() {
                for w in dag.descendants(result.index) {
                    if statuses[w] == NodeStatus::Queued {
                        statuses[w] = NodeStatus::Skipped;
                        skip_node(&self.state, &self.data, self.nodes[w].id).await;
                    }
                }
            }
        }
        statuses
            .iter()
            .map(|s| {
                if *s == NodeStatus::Queued {
                    NodeStatus::Cancelled
                } else {
                    *s
                }
            })
            .collect()
    }

    /// A node whose attempt the takeover interrupted: queued again for its
    /// next attempt, with an `interrupted` span on the trace, or failed when
    /// that was the last of its `budget`. In a run cancelled meanwhile it is
    /// left for the run's end to cancel. Returns its status now.
    async fn interrupted(&self, resume: &Resume, stored: &StoredNode, budget: i32) -> NodeStatus {
        let attempt = stored.attempt;
        let cancelled = self.token.is_cancelled();
        let last = attempt >= budget;
        let reason = if cancelled {
            format!(
                "attempt {attempt} was interrupted when the instance executing it stopped; the run was cancelled meanwhile"
            )
        } else if last {
            format!(
                "attempt {attempt} was interrupted when the instance executing it stopped; it was the last of {budget}"
            )
        } else {
            format!(
                "attempt {attempt} was interrupted when the instance executing it stopped; attempt {} follows",
                attempt + 1
            )
        };
        self.takeover_span(
            resume,
            stored,
            INTERRUPTED_DECISION,
            "interrupted",
            &reason,
            json!({
                "attempt": attempt,
                "next_attempt": (!last && !cancelled).then_some(attempt + 1),
                "max_attempts": budget,
                "handoffs": stored.handoffs,
            }),
        );
        if cancelled {
            return NodeStatus::Queued;
        }
        if last {
            fail_node(&self.state, &self.data, stored.node_id, attempt, reason).await;
            return NodeStatus::Failed;
        }
        let requeued = async {
            let mut tx = lease::begin(&self.state, &self.data.lease, "node requeue").await?;
            repo::runs::requeue_node(&mut tx, self.data.run_id, stored.node_id).await?;
            tx.commit().await?;
            Ok::<(), WriteError>(())
        }
        .await;
        match requeued {
            Ok(()) => publish_status(
                &self.state,
                &self.data,
                stored.node_id,
                NodeStatus::Queued,
                attempt,
                None,
                false,
            ),
            Err(err) => write_failed(&self.data, stored.node_id, "requeue", &err),
        }
        NodeStatus::Queued
    }

    /// A node that waited for an approval when the run was taken over: it
    /// waits here, with a `reattached` span on the trace.
    fn reattached(&self, resume: &Resume, stored: &StoredNode) {
        self.takeover_span(
            resume,
            stored,
            REATTACHED_DECISION,
            "reattached",
            "the node waited for an approval when the instance executing it stopped; it waits here for the decision",
            json!({"attempt": stored.attempt, "next_attempt": stored.attempt + 1}),
        );
    }

    /// The `policy` span a takeover puts on a node it picks up.
    fn takeover_span(
        &self,
        resume: &Resume,
        stored: &StoredNode,
        decision: &str,
        name: &str,
        reason: &str,
        extra: Value,
    ) {
        let now = Utc::now();
        let mut attributes = json!({
            "decision": decision,
            "rule": LEASE_RULE,
            "reason": reason,
            "action": "takeover",
            "previous_instance": resume.previous,
            "instance": self.data.lease.instance(),
            "lease_epoch": self.data.lease.epoch(),
            "released": resume.released,
        });
        if let (Value::Object(into), Value::Object(from)) = (&mut attributes, extra) {
            into.extend(from);
        }
        self.data.trace.emit(SpanDraft {
            id: Uuid::now_v7(),
            node_id: stored.node_id,
            agent_id: stored.agent_id,
            principal: SYSTEM_PRINCIPAL.to_owned(),
            parent_id: None,
            kind: SpanKind::Policy,
            name: name.to_owned(),
            started_at: now,
            ended_at: Some(now),
            status: if decision == INTERRUPTED_DECISION {
                SpanStatus::Error
            } else {
                SpanStatus::Ok
            },
            attributes,
        });
    }

    /// Outputs of upstream nodes outside the selection, from earlier runs.
    async fn initial_outputs(&self) -> HashMap<Uuid, String> {
        let selected: HashSet<Uuid> = self.nodes.iter().map(|n| n.id).collect();
        let upstream: HashSet<Uuid> = self
            .data
            .edges
            .iter()
            .filter(|e| e.blocking && selected.contains(&e.target) && !selected.contains(&e.source))
            .map(|e| e.source)
            .collect();
        // One read for all of them, not one per upstream node.
        let ids: Vec<Uuid> = upstream.into_iter().collect();
        repo::nodes::find_many(&self.state.db, self.data.graph_id, &ids)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|node| node.output.map(|output| (node.id, output)))
            .collect()
    }
}

/// Upstream results of `node` that are available, ordered by node id.
fn upstream_of(
    node: &GraphNode,
    edges: &[GraphEdge],
    outputs: &HashMap<Uuid, String>,
) -> Vec<(Uuid, String)> {
    let mut up: Vec<(Uuid, String)> = edges
        .iter()
        .filter(|e| e.blocking && e.target == node.id)
        .filter_map(|e| outputs.get(&e.source).map(|o| (e.source, o.clone())))
        .collect();
    up.sort_by_key(|(id, _)| *id);
    up
}

/// Watches the run at once and then every second: a cancel asked for on
/// any instance, the run changing hands (the lease is lost at once rather
/// than at the next write or renewal), agents quarantined elsewhere,
/// operators' commands.
fn spawn_cancel_watcher(
    state: AppState,
    lease: Lease,
    token: CancellationToken,
) -> CancellationToken {
    let done = CancellationToken::new();
    let stop = done.clone();
    let run_id = lease.run_id();
    tokio::spawn(async move {
        // The first look is at once: a cancel asked for before the run started (or was taken
        // over) here stops it before its nodes get far.
        let mut pause = Duration::ZERO;
        loop {
            tokio::select! {
                _ = stop.cancelled() => return,
                _ = token.cancelled() => return,
                _ = lease.stopped() => return,
                _ = tokio::time::sleep(pause) => {}
            }
            pause = CANCEL_POLL;
            match repo::runs::watched(&state.db, run_id).await {
                Ok(Some(watched)) => {
                    if let Some(epoch) = lease.epoch()
                        && (watched.lease_epoch != epoch
                            || watched.claimed_by != Some(lease.instance()))
                    {
                        give_up(
                            &state,
                            &lease,
                            &format!(
                                "the run is held at epoch {} by another instance",
                                watched.lease_epoch
                            ),
                        )
                        .await;
                        return;
                    }
                    if watched.cancel_requested {
                        token.cancel();
                        return;
                    }
                }
                Ok(None) => {
                    // The run is gone (its graph was deleted): nothing to write any more.
                    lease.lose();
                    return;
                }
                Err(err) => tracing::debug!(%run_id, error = %err, "run not watched this time"),
            }
            // An agent quarantined on another instance: its nodes here stop too.
            if let Ok(agents) = repo::runs::quarantined_agents_running(&state.db, run_id).await {
                for agent in agents {
                    state.engine.quarantine_local(agent);
                }
            }
            // An operator's command recorded on another instance (the cancel of a sub-agent).
            super::commands::deliver(&state, run_id).await;
        }
    });
    done
}

fn publish_status(
    state: &AppState,
    data: &RunData,
    node_id: Uuid,
    status: NodeStatus,
    attempt: i32,
    error: Option<String>,
    cached: bool,
) {
    state.hub.publish(
        data.graph_id,
        SseEvent::NodeStatus(NodeStatusEvent {
            run_id: data.run_id,
            node_id,
            status,
            attempt,
            error,
            cached,
        }),
    );
}

async fn skip_node(state: &AppState, data: &RunData, node_id: Uuid) {
    let error = "an upstream node failed";
    let outcome = NodeOutcome {
        status: NodeStatus::Skipped,
        attempt: 0,
        tokens_in: 0,
        tokens_out: 0,
        cost_usd: 0.0,
        cached: false,
        error: Some(error),
        output: None,
        content_hash: None,
        evidence: None,
    };
    match record(state, data, node_id, &outcome, None, 0, None).await {
        Ok(()) => publish_status(
            state,
            data,
            node_id,
            NodeStatus::Skipped,
            0,
            Some(error.into()),
            false,
        ),
        Err(err) => write_failed(data, node_id, "skipped node", &err),
    }
}

/// Fails a node without running it (it was interrupted on its last attempt,
/// or the approval it waited for expired), with `reason`.
async fn fail_node(state: &AppState, data: &RunData, node_id: Uuid, attempt: i32, reason: String) {
    let outcome = NodeOutcome {
        status: NodeStatus::Failed,
        attempt,
        tokens_in: 0,
        tokens_out: 0,
        cost_usd: 0.0,
        cached: false,
        error: Some(&reason),
        output: None,
        content_hash: None,
        evidence: None,
    };
    match record(state, data, node_id, &outcome, None, 0, None).await {
        Ok(()) => publish_status(
            state,
            data,
            node_id,
            NodeStatus::Failed,
            attempt,
            Some(reason.clone()),
            false,
        ),
        Err(err) => write_failed(data, node_id, "failed node", &err),
    }
}

/// A write of the executor did not happen. Refused: another instance holds
/// the run now, and this one stops (the lease says so already). Failed: the
/// run stops here too rather than going on from a state the database does
/// not have; its lease runs out and whoever takes it over resumes it from
/// what was written.
fn write_failed(data: &RunData, node_id: Uuid, what: &str, err: &WriteError) {
    match err {
        WriteError::Fenced => {
            tracing::debug!(run_id = %data.run_id, %node_id, what, "write refused: the run changed hands")
        }
        WriteError::Db(err) => {
            tracing::error!(run_id = %data.run_id, %node_id, what, error = %err,
                "write failed; the run stops here and resumes where it was written");
            data.lease.lose();
        }
    }
}

/// Persists a node outcome in one transaction under the run's lease: the
/// node run, the run totals, the agent's spend, the node's status and the
/// usage ledger entry. Nothing of it is written when the lease moved on.
async fn record(
    state: &AppState,
    data: &RunData,
    node_id: Uuid,
    outcome: &NodeOutcome<'_>,
    agent: Option<&Agent>,
    context_chars_saved: i64,
    span_id: Option<Uuid>,
) -> Result<(), WriteError> {
    // Tokens spent are added to the run's totals: the run row itself changes.
    let mut tx = if !outcome.cached && (outcome.tokens_in > 0 || outcome.tokens_out > 0) {
        lease::begin_run_update(state, &data.lease, "node outcome").await?
    } else {
        lease::begin(state, &data.lease, "node outcome").await?
    };
    repo::runs::finish_node(&mut tx, data.run_id, node_id, outcome).await?;
    if let Some(agent) = agent
        && !outcome.cached
    {
        repo::agents::add_spent(&mut *tx, agent.id, outcome.tokens_in + outcome.tokens_out).await?;
    }
    repo::nodes::set_status(&mut *tx, node_id, outcome.status, outcome.output).await?;
    if let Some(event) = spend_of(data, node_id, outcome, agent, context_chars_saved, span_id) {
        repo::usage::insert(&mut *tx, &event).await?;
    }
    tx.commit().await?;
    if let Some(agent) = agent
        && !outcome.cached
    {
        // The card shows what the agent spent.
        state
            .caches
            .invalidate(crate::app::caches::Key::Agent(agent.id));
    }
    state
        .metrics
        .node_runs
        .add(&[("status", outcome.status.as_str())], 1);
    Ok(())
}

/// The usage ledger entry of a node outcome that spent tokens at the provider.
fn spend_of(
    data: &RunData,
    node_id: Uuid,
    outcome: &NodeOutcome<'_>,
    agent: Option<&Agent>,
    context_chars_saved: i64,
    span_id: Option<Uuid>,
) -> Option<UsageEvent> {
    if outcome.cached || (outcome.tokens_in <= 0 && outcome.tokens_out <= 0) {
        return None;
    }
    // Same rule as `ExecContext::agent_target`: an agent's own model applies on Anthropic.
    let model = agent
        .filter(|a| {
            data.target.provider == LlmProviderKind::Anthropic && a.model.starts_with("claude-")
        })
        .map_or(data.target.model.as_str(), |a| a.model.as_str());
    let mut event = data.usage_event(
        UsagePurpose::Node,
        model,
        outcome.tokens_in,
        outcome.tokens_out,
        outcome.cost_usd,
        context_chars_saved,
    );
    event.agent_id = agent.map(|a| a.id);
    event.node_id = Some(node_id);
    event.span_id = span_id;
    Some(event)
}

/// Books what a node attempt spent at its provider when its outcome was
/// refused because another instance holds the run now: the tokens were
/// spent all the same, so they go to the usage ledger and the agent's
/// budget (outside the fence), though not to the run's totals, which count
/// the attempts of whoever holds the run.
async fn book_refused_spend(
    state: &AppState,
    data: &RunData,
    node_id: Uuid,
    outcome: &NodeOutcome<'_>,
    agent: Option<&Agent>,
    context_chars_saved: i64,
    span_id: Option<Uuid>,
) {
    let Some(event) = spend_of(data, node_id, outcome, agent, context_chars_saved, span_id) else {
        return;
    };
    let booked = async {
        let mut tx = state.db.begin().await?;
        repo::usage::insert(&mut *tx, &event).await?;
        if let Some(agent) = agent {
            repo::agents::add_spent(&mut *tx, agent.id, outcome.tokens_in + outcome.tokens_out)
                .await?;
        }
        tx.commit().await
    }
    .await;
    match booked {
        Ok(()) => {
            if let Some(agent) = agent {
                state
                    .caches
                    .invalidate(crate::app::caches::Key::Agent(agent.id));
            }
        }
        Err(err) => {
            tracing::error!(run_id = %data.run_id, %node_id, error = %err, "spend of a refused attempt not booked")
        }
    }
}

/// Content hash used for memoization: everything that influences the result.
fn content_hash(
    data: &RunData,
    vocabulary: &Ontology,
    node: &GraphNode,
    upstream: &[(Uuid, String)],
) -> String {
    let upstream: Vec<_> = upstream
        .iter()
        .map(|(id, out)| json!([id, kernel::sha256_hex(out.as_bytes())]))
        .collect();
    let material = json!({
        "v": 1,
        "goal": data.goal,
        "title": node.title,
        "content": node.content,
        "kind": node.kind,
        "type": vocabulary.node_type(&node.kind),
        "executor": node.executor,
        "agent_role": node.agent_role,
        "provider": data.target.provider,
        "model": data.target.model,
        "upstream": upstream,
    });
    kernel::sha256_hex(material.to_string().as_bytes())
}

/// Runs one node from `first_attempt` on, up to its `budget`th attempt;
/// `carried` is the decision taken on an approval the node waited for when
/// its run was taken over.
#[allow(clippy::too_many_arguments)]
async fn run_node(
    state: AppState,
    data: Arc<RunData>,
    token: CancellationToken,
    semaphore: Arc<Semaphore>,
    index: usize,
    node: GraphNode,
    upstream: Vec<(Uuid, String)>,
    first_attempt: i32,
    budget: i32,
    carried: Option<Approval>,
) -> NodeResult {
    let permit = tokio::select! {
        _ = token.cancelled() => None,
        p = semaphore.acquire_owned() => p.ok(),
    };
    let Some(_permit) = permit else {
        return NodeResult {
            index,
            status: NodeStatus::Cancelled,
            output: None,
        };
    };
    let node_id = node.id;
    // Attempts spent waiting for the vocabulary count against the node's budget.
    let (vocabulary, first_attempt) =
        match node_vocabulary(&state, &data, &token, node_id, first_attempt, budget).await {
            Ok(found) => found,
            Err(status) => {
                return NodeResult {
                    index,
                    status,
                    output: None,
                };
            }
        };
    match NodeTask::new(
        state.clone(),
        data.clone(),
        token,
        node,
        upstream,
        carried,
        vocabulary,
    )
    .await
    {
        Ok(task) => task.run(index, first_attempt, budget).await,
        Err(err) => {
            let message = err.to_string();
            let attempt = first_attempt - 1;
            let outcome = NodeOutcome {
                status: NodeStatus::Failed,
                attempt,
                tokens_in: 0,
                tokens_out: 0,
                cost_usd: 0.0,
                cached: false,
                error: Some(&message),
                output: None,
                content_hash: None,
                evidence: None,
            };
            match record(&state, &data, node_id, &outcome, None, 0, None).await {
                Ok(()) => publish_status(
                    &state,
                    &data,
                    node_id,
                    NodeStatus::Failed,
                    attempt,
                    Some(message),
                    false,
                ),
                Err(err) => write_failed(&data, node_id, "node failure", &err),
            }
            NodeResult {
                index,
                status: NodeStatus::Failed,
                output: None,
            }
        }
    }
}

/// The graph's vocabulary for a node about to run: the one the run started
/// with, or asked of onto-intelli again. While onto-intelli cannot be
/// reached and nothing is kept, the node fails with a retryable "ontology
/// unavailable" error: it is tried again with backoff within its attempts,
/// and fails for good after the last. Any other refusal fails it at once.
/// Returns the attempt the node runs as: the attempts spent waiting count
/// against its budget.
async fn node_vocabulary(
    state: &AppState,
    data: &RunData,
    token: &CancellationToken,
    node_id: Uuid,
    first_attempt: i32,
    budget: i32,
) -> Result<(Arc<Ontology>, i32), NodeStatus> {
    if let Some(vocabulary) = &data.ontology {
        return Ok((vocabulary.clone(), first_attempt));
    }
    let graph = vocabulary::GraphRef {
        id: data.graph_id,
        workspace_id: data.workspace_id,
        name: &data.graph_name,
    };
    let max_attempts = budget.max(1);
    let first_attempt = first_attempt.clamp(1, max_attempts);
    for attempt in first_attempt..=max_attempts {
        let error = match vocabulary::of(state, graph, Actor::Runtime).await {
            Ok(vocabulary) => return Ok((vocabulary, attempt)),
            Err(err @ VocabularyError::Unavailable(_)) => ExecError::transient(err.to_string()),
            Err(err) => ExecError::fatal(err.to_string()),
        };
        if !error.retryable || attempt == max_attempts {
            fail_node(state, data, node_id, attempt, error.message).await;
            return Err(NodeStatus::Failed);
        }
        let delay = full_jitter(
            attempt as u32,
            Duration::from_secs(2),
            Duration::from_secs(60),
        );
        tracing::warn!(run_id = %data.run_id, %node_id, attempt, error = %error.message,
            "attempt {attempt} failed: the graph's vocabulary is not at hand; retrying in {:.1}s",
            delay.as_secs_f64());
        tokio::select! {
            _ = token.cancelled() => return Err(NodeStatus::Cancelled),
            _ = tokio::time::sleep(delay) => {}
        }
    }
    unreachable!("the last attempt always returns")
}

/// A node that waited for an approval during `attempt` when its run was
/// taken over: it waits here for the decision, then runs its next attempt
/// with the decision carried (the runtime asks again and is answered at
/// once). An expired request fails it; a cancel of the run cancels it.
#[allow(clippy::too_many_arguments)]
async fn resume_waiting(
    state: AppState,
    data: Arc<RunData>,
    token: CancellationToken,
    semaphore: Arc<Semaphore>,
    index: usize,
    node: GraphNode,
    upstream: Vec<(Uuid, String)>,
    attempt: i32,
    budget: i32,
) -> NodeResult {
    let node_id = node.id;
    let reattached = approvals::reattach(
        &state,
        &data.trace,
        &data.lease,
        &token,
        data.run_id,
        node_id,
    )
    .await;
    let carried = match reattached {
        Reattached::Decided(approval) => Some(approval),
        Reattached::Missing => None,
        // The run stops here: the node is left waiting for whoever holds the run next.
        Reattached::Stopped => {
            return NodeResult {
                index,
                status: NodeStatus::WaitingApproval,
                output: None,
            };
        }
        Reattached::Expired(approval) => {
            let reason = format!(
                "the approval of {} expired while the node waited",
                approval.tool
            );
            fail_node(&state, &data, node_id, attempt, reason).await;
            return NodeResult {
                index,
                status: NodeStatus::Failed,
                output: None,
            };
        }
        Reattached::Cancelled => {
            let outcome = NodeOutcome {
                status: NodeStatus::Cancelled,
                attempt,
                tokens_in: 0,
                tokens_out: 0,
                cost_usd: 0.0,
                cached: false,
                error: None,
                output: None,
                content_hash: None,
                evidence: None,
            };
            match record(&state, &data, node_id, &outcome, None, 0, None).await {
                Ok(()) => publish_status(
                    &state,
                    &data,
                    node_id,
                    NodeStatus::Cancelled,
                    attempt,
                    None,
                    false,
                ),
                Err(err) => write_failed(&data, node_id, "cancelled node", &err),
            }
            return NodeResult {
                index,
                status: NodeStatus::Cancelled,
                output: None,
            };
        }
    };
    if attempt >= budget {
        let reason = format!(
            "attempt {attempt} was interrupted while it waited for an approval; it was the last of {budget}"
        );
        fail_node(&state, &data, node_id, attempt, reason).await;
        return NodeResult {
            index,
            status: NodeStatus::Failed,
            output: None,
        };
    }
    run_node(
        state,
        data,
        token,
        semaphore,
        index,
        node,
        upstream,
        attempt + 1,
        budget,
        carried,
    )
    .await
}

/// What the context builder kept from an agent, and why.
#[derive(Default)]
struct Withheld {
    chunk_ids: Vec<Uuid>,
    memory_ids: Vec<Uuid>,
    by_labels: usize,
    by_topics: usize,
    by_namespaces: usize,
    first_rule: Option<&'static str>,
}

impl Withheld {
    /// Whether `action` is allowed; records the id when it is not.
    fn admits(&mut self, caps: &Capabilities, action: Action<'_>, id: Uuid, passage: bool) -> bool {
        let decision = policy::decide(caps, &action);
        if decision.allowed {
            return true;
        }
        if passage {
            self.chunk_ids.push(id);
        } else {
            self.memory_ids.push(id);
        }
        match decision.rule {
            "data.labels" => self.by_labels += 1,
            "data.topics" => self.by_topics += 1,
            _ => self.by_namespaces += 1,
        }
        self.first_rule.get_or_insert(decision.rule);
        false
    }

    /// The attributes of the `policy` span listing what was withheld, if anything was.
    fn attributes(&self, policy_version: i32) -> Option<Value> {
        let rule = self.first_rule?;
        let count = self.chunk_ids.len() + self.memory_ids.len();
        Some(json!({
            "decision": "deny",
            "rule": rule,
            "action": "data_read",
            "reason": format!("{count} item(s) outside the agent's data scopes were withheld"),
            "policy_version": policy_version,
            "withheld_document_chunk_ids": self.chunk_ids,
            "withheld_memory_ids": self.memory_ids,
            "withheld_by_labels": self.by_labels,
            "withheld_by_topics": self.by_topics,
            "withheld_by_namespaces": self.by_namespaces,
        }))
    }
}

/// How one attempt of a node ended.
enum Attempt {
    /// The run was cancelled.
    Cancelled,
    /// The node's agent was quarantined.
    Quarantined,
    /// The executor answered, or the attempt timed out.
    Done(Result<Result<ExecOutput, ExecError>, tokio::time::error::Elapsed>),
}

/// One node's execution: cache lookup, budget check, attempts with retries.
struct NodeTask {
    ctx: ExecContext,
    data: Arc<RunData>,
    token: CancellationToken,
    hash: String,
    /// Characters of upstream output that were left out of the prompt.
    context_saved: i64,
}

impl NodeTask {
    async fn new(
        state: AppState,
        data: Arc<RunData>,
        token: CancellationToken,
        node: GraphNode,
        upstream: Vec<(Uuid, String)>,
        carried: Option<Approval>,
        vocabulary: Arc<Ontology>,
    ) -> anyhow::Result<Self> {
        let began = Utc::now();
        let hash = content_hash(&data, &vocabulary, &node, &upstream);
        let agent = match (data.pinned_agent, data.workspace_id) {
            (Some(pinned), Some(workspace)) => {
                repo::agents::find(&state.db, workspace, pinned).await?
            }
            _ => planner::assign(&state, data.workspace_id, &node, &vocabulary).await?,
        };
        let query = format!("{} {}", node.title, node.content);
        let recalled = match data.workspace_id {
            Some(workspace) => memory::retrieve(
                &state.memories,
                &state.db,
                data.owner,
                workspace,
                memory::View::Prefer(data.graph_id),
                &query,
                5,
            )
            .await
            .unwrap_or_default(),
            None => Vec::new(),
        };
        let passages = match data.workspace_id {
            Some(workspace) => {
                knowledge::context_passages(
                    &state,
                    workspace,
                    data.owner,
                    &query,
                    knowledge::Use::Node,
                )
                .await
            }
            None => Vec::new(),
        };
        // The policies in force, resolved for this agent (workspace -> team -> agent over its
        // own capabilities), decide what reaches it; a node without an agent reads as the system
        // does. What was withheld is on the record before the node runs, ids only.
        let levels = policies::levels_for(
            &data.policies,
            agent.as_ref().and_then(|a| a.team_id),
            agent.as_ref().map(|a| a.id),
        );
        let resolved = policies::resolve(&levels, agent.as_ref().map(|a| &a.capabilities));
        let (capabilities, rules) = (resolved.capabilities, resolved.rules);
        let policy_version = agent.as_ref().map_or(0, |a| a.policy_version);
        let mut withheld = Withheld::default();
        let recalled: Vec<_> = recalled
            .into_iter()
            .filter(|m| {
                agent.is_none()
                    || withheld.admits(
                        &capabilities,
                        Action::Read {
                            label: m.label,
                            topic: None,
                            namespace: Some(m.scope),
                        },
                        m.id,
                        false,
                    )
            })
            .collect();
        let passages: Vec<_> = passages
            .into_iter()
            .filter(|p| {
                agent.is_none()
                    || withheld.admits(
                        &capabilities,
                        Action::Read {
                            label: p.label,
                            topic: p.topic_id,
                            namespace: None,
                        },
                        p.chunk_id,
                        true,
                    )
            })
            .collect();
        let memory_ids: Vec<Uuid> = recalled.iter().map(|m| m.id).collect();
        // What the node produces is derived from what it reads: its own context and its
        // upstream nodes' (whatever earlier run they were given it in).
        let upstream_ids: Vec<Uuid> = upstream.iter().map(|(id, _)| *id).collect();
        let mut labels_given: Vec<DataLabel> = recalled
            .iter()
            .map(|m| m.label)
            .chain(passages.iter().map(|p| p.label))
            .chain(
                repo::runs::context_labels_of(&state.db, &upstream_ids)
                    .await
                    .unwrap_or_default(),
            )
            .collect();
        labels_given.sort_by_key(|l| l.rank());
        labels_given.dedup();
        // Every piece of context carries the id the model is told to cite it by.
        // What the recording replaces by its citation: the passages and memories of the
        // labels the workspace redacts, as the agent is given them.
        let redact = &data.policy.recording.redact_labels;
        let mut redactions: Vec<Redaction> = Vec::new();
        let memories: Vec<String> = recalled
            .into_iter()
            .map(|m| {
                let text = data.policy.scrub(m.content);
                if redact.contains(&m.label) {
                    redactions.push(Redaction::memory(m.id, m.label, text.clone()));
                }
                evidence::memory_line(m.id, &text)
            })
            .collect();
        let chunk_ids: Vec<Uuid> = passages.iter().map(|p| p.chunk_id).collect();
        let documents: Vec<String> = passages
            .into_iter()
            .map(|p| {
                let text = data.policy.scrub(p.content);
                if redact.contains(&p.label) {
                    redactions.push(Redaction::passage(p.chunk_id, p.label, text.clone()));
                }
                format!(
                    "{}\n{}",
                    evidence::document_head(p.chunk_id, &p.citation),
                    text
                )
            })
            .collect();
        let principal = agent
            .as_ref()
            .map_or_else(|| SYSTEM_PRINCIPAL.to_owned(), |a| a.principal_id.clone());
        if let Some(attributes) = withheld.attributes(policy_version) {
            data.trace.emit(SpanDraft {
                id: Uuid::now_v7(),
                node_id: node.id,
                agent_id: agent.as_ref().map(|a| a.id),
                principal: principal.clone(),
                parent_id: None,
                kind: SpanKind::Policy,
                name: "context".into(),
                started_at: began,
                ended_at: Some(Utc::now()),
                status: SpanStatus::Error,
                attributes,
            });
        }
        // An agent works over several turns and takes more context than one LLM call.
        let budget = match node.executor {
            Executor::Agent => AGENT_UPSTREAM_CHARS,
            _ => UPSTREAM_CHARS,
        };
        let mut context_saved = 0;
        // The titles of all upstream nodes in one read, not one per node.
        let titles: HashMap<Uuid, String> =
            repo::nodes::find_many(&state.db, data.graph_id, &upstream_ids)
                .await?
                .into_iter()
                .map(|n| (n.id, n.title))
                .collect();
        let mut upstream_outputs = Vec::with_capacity(upstream.len());
        for (id, output) in upstream {
            let fitted = context::fit(&output, &query, budget);
            context_saved += fitted.saved_chars() as i64;
            let output = data.policy.scrub(fitted.text);
            let title = titles.get(&id).cloned().unwrap_or_default();
            let edge = data
                .edges
                .iter()
                .find(|e| e.blocking && e.source == id && e.target == node.id);
            upstream_outputs.push(UpstreamOutput {
                node_id: id,
                title,
                output,
                relation: edge
                    .and_then(|e| vocabulary.relation_type(&e.kind))
                    .map(|r| r.label.clone())
                    .unwrap_or_default(),
                reason: edge.map(|e| e.reason.clone()).unwrap_or_default(),
            });
        }
        // Data about to go to the model is screened for instructions aimed at it: a finding
        // is a `policy` span, and under `injection.action: block` the piece is withheld.
        let block = rules.injection.action == InjectionAction::Block;
        let mut findings: Vec<(&str, String, InjectionFinding)> = Vec::new();
        for output in &mut upstream_outputs {
            if let Some(finding) = injection::screen(&output.output) {
                if block {
                    output.output = injection::withheld_notice("upstream output", &finding);
                }
                findings.push(("upstream", output.node_id.to_string(), finding));
            }
        }
        let screened =
            |items: Vec<String>,
             ids: &[Uuid],
             source: &'static str,
             findings: &mut Vec<(&str, String, InjectionFinding)>| {
                items
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, text)| match injection::screen(&text) {
                        Some(finding) => {
                            let subject = ids.get(i).map(ToString::to_string).unwrap_or_default();
                            findings.push((source, subject, finding));
                            (!block).then_some(text)
                        }
                        None => Some(text),
                    })
                    .collect::<Vec<String>>()
            };
        let memories = screened(memories, &memory_ids, "memory", &mut findings);
        let documents = screened(documents, &chunk_ids, "document", &mut findings);
        for (source, subject, finding) in &findings {
            let reason = format!(
                "instructions aimed at the model in {source} {subject}: {}",
                finding.describe()
            );
            let decision = if block {
                Decision::deny("injection", reason.clone())
            } else {
                Decision::alert("injection", finding.severity, reason.clone())
            };
            let span_id = Uuid::now_v7();
            data.trace.emit(SpanDraft {
                id: span_id,
                node_id: node.id,
                agent_id: agent.as_ref().map(|a| a.id),
                principal: principal.clone(),
                parent_id: None,
                kind: SpanKind::Policy,
                name: "injection".into(),
                started_at: began,
                ended_at: Some(Utc::now()),
                status: if block {
                    SpanStatus::Error
                } else {
                    SpanStatus::Ok
                },
                attributes: json!({
                    "decision": decision.verdict(),
                    "rule": "injection",
                    "reason": reason,
                    "action": "context",
                    "source": source,
                    "subject": subject,
                    "severity": finding.severity,
                    "patterns": finding.patterns,
                    "hidden_chars": finding.hidden_chars,
                    "policy_version": policy_version,
                    "policy_versions": rules.versions,
                }),
            });
            if finding.severity == Severity::High
                && let Some(workspace_id) = data.workspace_id
            {
                super::defend::raise(
                    &state,
                    &About {
                        workspace_id,
                        agent_id: agent.as_ref().map(|a| a.id),
                        principal: &principal,
                        run_id: Some(data.run_id),
                        node_id: Some(node.id),
                        graph_id: Some(data.graph_id),
                        actor: Some(data.owner),
                    },
                    &Finding {
                        span_id: Some(span_id),
                        kind: AlertKind::Injection,
                        severity: finding.severity,
                        rule: "injection",
                        title: &format!("instructions found in a {source} given to {}", node.title),
                        detail: &format!(
                            "{}{}",
                            finding.describe(),
                            if block {
                                "; withheld"
                            } else {
                                "; let through, flagged"
                            }
                        ),
                    },
                )
                .await;
            }
        }
        // What the node was given, by id: what its citations are checked against. An id
        // written inside that text (an upstream output citing its sources) is seen, not given.
        let mut given = Given {
            documents: chunk_ids.clone(),
            memories: memory_ids.clone(),
            nodes: upstream_ids.clone(),
            facts: Vec::new(),
            seen: Vec::new(),
        };
        for text in upstream_outputs
            .iter()
            .map(|u| u.output.as_str())
            .chain(memories.iter().map(String::as_str))
            .chain(documents.iter().map(String::as_str))
        {
            given.see(text);
        }
        // What the node was given to read is on the record before it runs.
        data.trace.emit(SpanDraft {
            id: Uuid::now_v7(),
            node_id: node.id,
            agent_id: agent.as_ref().map(|a| a.id),
            principal,
            parent_id: None,
            kind: SpanKind::DataRead,
            name: "context".into(),
            started_at: began,
            ended_at: Some(Utc::now()),
            status: SpanStatus::Ok,
            attributes: json!({
                "document_chunk_ids": chunk_ids,
                "memory_ids": memory_ids,
                "upstream_node_ids": upstream_ids,
                "citation_ids": given.keys(),
                "documents": chunk_ids.len(),
                "memories": memory_ids.len(),
                "chars_saved": context_saved,
                "labels": labels_given,
                "policy_versions": rules.versions,
                "recording": data.policy.recording.is_on(),
                "recording_kinds": data.policy.recording.kinds(),
                "recording_redact_labels": data.policy.recording.redact_labels,
                // The human the node's agent acts for, as its token names them.
                "on_behalf_of": OnBehalfOf {
                    user_id: data.owner,
                    workspace_role: data.owner_role,
                },
            }),
        });
        let ctx = ExecContext {
            state,
            run_id: data.run_id,
            graph_id: data.graph_id,
            workspace_id: data.workspace_id,
            owner: data.owner,
            owner_role: data.owner_role,
            goal: data.goal.clone(),
            node_type: vocabulary.node_type(&node.kind).cloned(),
            code_exec_allowed: data.policy.allow_code_exec,
            node,
            upstream: upstream_outputs,
            memories,
            documents,
            given,
            agent,
            capabilities,
            policy_version,
            rules,
            labels_given,
            target: data.target.clone(),
            force: data.force,
            trace: data.trace.clone(),
            attempt: std::sync::atomic::AtomicI32::new(0),
            recording: data.policy.recording.clone(),
            scrub_secrets: data.policy.redact_secrets,
            redactions,
            lease: data.lease.clone(),
            carried: Arc::new(Mutex::new(carried.into_iter().collect())),
        };
        Ok(NodeTask {
            ctx,
            data,
            token,
            hash,
            context_saved,
        })
    }

    /// Runs the node from `first_attempt` on (later than 1 when an earlier
    /// attempt was interrupted by a takeover), up to its `budget`th attempt
    /// (`NEXC_MAX_ATTEMPTS`, plus the attempts a hand-off cut short).
    async fn run(self, index: usize, first_attempt: i32, budget: i32) -> NodeResult {
        let status_and_output = match self.serve_cached().await {
            Some(output) => (NodeStatus::Succeeded, Some(output)),
            None => self.attempts(first_attempt, budget).await,
        };
        NodeResult {
            index,
            status: status_and_output.0,
            output: status_and_output.1,
        }
    }

    async fn serve_cached(&self) -> Option<String> {
        if self.data.force {
            return None;
        }
        let state = &self.ctx.state;
        let node_id = self.ctx.node.id;
        let hit = repo::runs::find_cached(&state.db, self.data.graph_id, &self.hash)
            .await
            .ok()??;
        let outcome = NodeOutcome {
            status: NodeStatus::Succeeded,
            attempt: 0,
            tokens_in: hit.tokens_in,
            tokens_out: hit.tokens_out,
            cost_usd: 0.0,
            cached: true,
            error: None,
            output: Some(&hit.output),
            content_hash: Some(&self.hash),
            evidence: hit.evidence.clone(),
        };
        if let Err(err) = record(state, &self.data, node_id, &outcome, None, 0, None).await {
            write_failed(&self.data, node_id, "cached result", &err);
            return None;
        }
        if let Err(err) = artifacts::copy_from_run(
            state,
            &self.data.lease,
            self.data.graph_id,
            hit.run_id,
            self.data.run_id,
            node_id,
        )
        .await
        {
            self.ctx.log(
                LogLevel::Warn,
                format!("cached artifacts could not be copied: {err}"),
            );
        }
        state.metrics.node_cache_hits.add(1);
        self.ctx.output(&hit.output);
        publish_status(
            state,
            &self.data,
            node_id,
            NodeStatus::Succeeded,
            0,
            None,
            true,
        );
        Some(hit.output)
    }

    fn budget_error(&self) -> Option<String> {
        let agent = self.ctx.agent.as_ref()?;
        match agent.status {
            AgentStatus::OverBudget => {
                Some(format!("agent {} is over its token budget", agent.name))
            }
            AgentStatus::Paused => Some(format!("agent {} is paused", agent.name)),
            AgentStatus::Quarantined => Some(format!("agent {} is quarantined", agent.name)),
            AgentStatus::Active if agent.is_over_budget() => {
                Some(format!("agent {} is over its token budget", agent.name))
            }
            AgentStatus::Active => None,
        }
    }

    async fn attempts(&self, first_attempt: i32, budget: i32) -> (NodeStatus, Option<String>) {
        let state = &self.ctx.state;
        let settings = state.settings.clone();
        let max_attempts = budget.max(1);
        let first_attempt = first_attempt.clamp(1, max_attempts);
        // What failed before an attempt started counts the attempts already made.
        let before = first_attempt - 1;
        if let Some(reason) = self.budget_error() {
            return self
                .finish(NodeStatus::Failed, before, None, Some(reason))
                .await;
        }
        // The model allow-list is enforced here, before anything is dispatched.
        let model = self.ctx.agent_target().model;
        let decision = self.ctx.decide(&Action::Model(&model));
        if !decision.allowed {
            self.ctx
                .policy_span(&model, &decision, json!({"action": "model"}));
            let reason = format!("policy: {}", decision.reason);
            return self
                .finish(NodeStatus::Failed, before, None, Some(reason))
                .await;
        }
        // So are the rules only the backend can see: working hours, the day's spend, the pace.
        if let Some(decision) = self.gate().await {
            self.ctx
                .policy_span(decision.rule, &decision, json!({"action": "node"}));
            let reason = format!("policy: {}", decision.reason);
            return self
                .finish(NodeStatus::Failed, before, None, Some(reason))
                .await;
        }
        let executor = executor::for_kind(self.ctx.node.executor);
        let agent_id = self.ctx.agent.as_ref().map(|a| a.id);
        let node_id = self.ctx.node.id;
        for attempt in first_attempt..=max_attempts {
            if let Err(err) = self.mark_running(attempt, agent_id).await {
                // The run stops here (the lease moved on, or the database failed it).
                write_failed(&self.data, node_id, "node start", &err);
                return (NodeStatus::Cancelled, None);
            }
            // Quarantining the agent ends the attempt without ending the run.
            let stop = CancellationToken::new();
            if let Some(id) = agent_id {
                state.engine.agent_started(id, node_id, stop.clone());
            }
            let result = tokio::select! {
                _ = self.token.cancelled() => Attempt::Cancelled,
                _ = stop.cancelled() => Attempt::Quarantined,
                r = tokio::time::timeout(settings.node_timeout, executor.execute(&self.ctx)) => Attempt::Done(r),
            };
            if let Some(id) = agent_id {
                state.engine.agent_finished(id, node_id);
            }
            let error = match result {
                Attempt::Cancelled => {
                    return self
                        .finish(NodeStatus::Cancelled, attempt, None, None)
                        .await;
                }
                Attempt::Quarantined => {
                    let reason = format!(
                        "agent {} was quarantined while the node ran",
                        self.ctx.principal()
                    );
                    self.ctx.policy_span(
                        "quarantine",
                        &Decision::deny("quarantine", reason.clone()),
                        json!({"action": "node"}),
                    );
                    return self
                        .finish(NodeStatus::Failed, attempt, None, Some(reason))
                        .await;
                }
                Attempt::Done(Ok(Ok(out))) => {
                    return self
                        .finish(NodeStatus::Succeeded, attempt, Some(out), None)
                        .await;
                }
                Attempt::Done(Ok(Err(e))) => e,
                Attempt::Done(Err(_)) => ExecError::transient(format!(
                    "timed out after {}s",
                    settings.node_timeout.as_secs()
                )),
            };
            if !error.retryable || attempt == max_attempts {
                return self
                    .finish(NodeStatus::Failed, attempt, None, Some(error.message))
                    .await;
            }
            let delay = full_jitter(
                attempt as u32,
                Duration::from_secs(2),
                Duration::from_secs(60),
            );
            self.ctx.log(
                LogLevel::Warn,
                format!(
                    "attempt {attempt} failed: {}; retrying in {:.1}s",
                    error.message,
                    delay.as_secs_f64()
                ),
            );
            tokio::select! {
                _ = self.token.cancelled() => return self.finish(NodeStatus::Cancelled, attempt, None, None).await,
                _ = tokio::time::sleep(delay) => {}
            }
        }
        unreachable!("the last attempt always returns")
    }

    /// Why the node may not start now under the resolved rules, if it may not:
    /// outside the time windows, over the day's token or dollar ceiling, or
    /// past the agent's starts per hour.
    async fn gate(&self) -> Option<Decision> {
        let rules = &self.ctx.rules;
        let windows = rules.within_time_windows(Utc::now());
        if !windows.allowed {
            return Some(windows);
        }
        let agent = self.ctx.agent.as_ref()?;
        let limits = &rules.limits;
        if limits.tokens_per_day.is_some() || limits.cost_usd_per_day.is_some() {
            let (tokens, cost) = repo::usage::agent_today(&self.ctx.state.db, agent.id)
                .await
                .unwrap_or((0, 0.0));
            if let Some(ceiling) = limits.tokens_per_day
                && tokens >= ceiling
            {
                return Some(Decision::deny(
                    "limits.tokens_per_day",
                    format!("the agent spent {tokens} of its {ceiling} tokens for today"),
                ));
            }
            if let Some(ceiling) = limits.cost_usd_per_day
                && cost >= ceiling
            {
                return Some(Decision::deny(
                    "limits.cost_usd_per_day",
                    format!("the agent spent ${cost:.2} of its ${ceiling:.2} for today"),
                ));
            }
        }
        if let Some(per_hour) = limits.runs_per_hour
            && let Err(wait) = self.ctx.state.engine.admit_dispatch(agent.id, per_hour)
        {
            return Some(Decision::deny(
                "limits.runs_per_hour",
                format!(
                    "the agent already started {per_hour} node(s) this hour; next in {}s",
                    wait.as_secs()
                ),
            ));
        }
        None
    }

    async fn mark_running(&self, attempt: i32, agent_id: Option<Uuid>) -> Result<(), WriteError> {
        let state = &self.ctx.state;
        let node_id = self.ctx.node.id;
        self.ctx
            .attempt
            .store(attempt, std::sync::atomic::Ordering::Relaxed);
        let mut tx = lease::begin(state, &self.data.lease, "node start").await?;
        repo::runs::start_node(
            &mut *tx,
            self.data.run_id,
            node_id,
            attempt,
            agent_id,
            &self.ctx.rules.versions,
            &self.ctx.labels_given,
        )
        .await?;
        repo::nodes::set_status(&mut *tx, node_id, NodeStatus::Running, None).await?;
        tx.commit().await?;
        publish_status(
            state,
            &self.data,
            node_id,
            NodeStatus::Running,
            attempt,
            None,
            false,
        );
        Ok(())
    }

    /// The evidence verdict on a successful output (`domain::evidence`): the
    /// executor assessed it with its Markdown artifacts, else the output is
    /// assessed alone here. The verdict is the node's `policy` span named
    /// `evidence`, one per attempt: `allow` when verified, `alert` otherwise,
    /// and under a policy that requires evidence with `action: reject`,
    /// `deny`, which fails the node with the verdict as the reason and keeps
    /// the output from the graph. Returns the status and the error to finish with.
    fn verify(
        &self,
        status: NodeStatus,
        out: &mut Option<ExecOutput>,
        error: Option<String>,
    ) -> (NodeStatus, Option<String>) {
        let Some(output) = out.as_mut().filter(|_| status == NodeStatus::Succeeded) else {
            return (status, error);
        };
        let rule = &self.ctx.rules.evidence;
        let evidence = output.evidence.get_or_insert_with(|| {
            evidence::assess(
                &[output.output.as_str()],
                &self.ctx.given,
                rule.min_coverage,
            )
        });
        let reason = evidence.describe();
        let decision = match (evidence.meets(), rule.required, rule.action) {
            (true, _, _) => Decision::allow(evidence.rule(), reason),
            (false, true, EvidenceAction::Reject) => Decision::deny(evidence.rule(), reason),
            (false, _, _) => Decision::alert(evidence.rule(), Severity::Low, reason),
        };
        let mut extra = json!({
            "action": "evidence",
            "turn_span_id": output.span_id,
            "evidence_required": rule.required,
            "evidence_action": rule.action,
        });
        for (key, value) in evidence.span_attributes() {
            extra[key] = value;
        }
        self.ctx.policy_span("evidence", &decision, extra);
        if decision.allowed {
            return (status, error);
        }
        let message = format!("policy: evidence {}: {}", evidence.verdict, decision.reason);
        self.ctx.log(LogLevel::Warn, message.clone());
        (NodeStatus::Failed, Some(message))
    }

    async fn finish(
        &self,
        status: NodeStatus,
        attempt: i32,
        mut out: Option<ExecOutput>,
        error: Option<String>,
    ) -> (NodeStatus, Option<String>) {
        let state = &self.ctx.state;
        let node_id = self.ctx.node.id;
        let (status, error) = self.verify(status, &mut out, error);
        // An output the evidence policy rejected is on the record (its verdict, its tokens)
        // but is not handed on: not stored as the node's output, not built on downstream.
        let delivered = status == NodeStatus::Succeeded;
        let (tokens_in, tokens_out, tokens_cached) = out
            .as_ref()
            .map_or((0, 0, 0), |o| (o.tokens_in, o.tokens_out, o.tokens_cached));
        let target = self.ctx.agent_target();
        let cost = match target.provider {
            LlmProviderKind::Demo => 0.0,
            _ => cost_usd_cached(
                &target.model,
                tokens_in.max(0) as u64,
                tokens_out.max(0) as u64,
                tokens_cached.max(0) as u64,
            ),
        };
        let outcome = NodeOutcome {
            status,
            attempt,
            tokens_in,
            tokens_out,
            cost_usd: cost,
            cached: false,
            error: error.as_deref(),
            output: out
                .as_ref()
                .filter(|_| delivered)
                .map(|o| o.output.as_str()),
            content_hash: delivered.then_some(self.hash.as_str()),
            evidence: out
                .as_ref()
                .and_then(|o| o.evidence.as_ref())
                .and_then(|e| serde_json::to_value(e).ok()),
        };
        let span_id = out.as_ref().and_then(|o| o.span_id);
        let agent = self.ctx.agent.as_ref();
        if let Err(err) = record(
            state,
            &self.data,
            node_id,
            &outcome,
            agent,
            self.context_saved,
            span_id,
        )
        .await
        {
            // Not recorded, so not announced and not built on: whoever holds the run resumes it.
            // What the attempt spent at the provider is booked all the same.
            if err.is_fenced() {
                book_refused_spend(
                    state,
                    &self.data,
                    node_id,
                    &outcome,
                    agent,
                    self.context_saved,
                    span_id,
                )
                .await;
            }
            write_failed(&self.data, node_id, "node outcome", &err);
            return (NodeStatus::Cancelled, None);
        }
        publish_status(state, &self.data, node_id, status, attempt, error, false);
        let output = out.filter(|_| delivered).map(|o| o.output);
        if let Some(text) = output.as_ref() {
            self.remember(text.clone());
        }
        (status, output)
    }

    /// Extracts and consolidates memories from a successful output in the background.
    fn remember(&self, output: String) {
        let ctx = &self.ctx;
        let usable = !ctx.target.provider.requires_api_key() || ctx.target.api_key.is_some();
        if !usable || output.trim().is_empty() {
            return;
        }
        let (state, target, goal, node) = (
            ctx.state.clone(),
            ctx.target.clone(),
            ctx.goal.clone(),
            ctx.node.clone(),
        );
        let (owner, graph_id) = (self.data.owner, self.data.graph_id);
        let workspace = self.data.workspace_id;
        let data = self.data.clone();
        tokio::spawn(async move {
            let result = async {
                let model = target.model.clone();
                let (candidates, spent) =
                    memory::extract(&state.llm, target, &goal, &node, &output).await?;
                let cost = match data.target.provider {
                    LlmProviderKind::Demo => 0.0,
                    _ => cost_usd(&model, spent.input_tokens, spent.output_tokens),
                };
                let mut event = data.usage_event(
                    UsagePurpose::Memory,
                    &model,
                    spent.input_tokens as i64,
                    spent.output_tokens as i64,
                    cost,
                    0,
                );
                event.node_id = Some(node.id);
                usage::record(&state, event).await;
                memory::store(
                    &state.memories,
                    &state.db,
                    owner,
                    workspace,
                    graph_id,
                    &candidates,
                )
                .await
            }
            .await;
            if let Err(err) = result {
                tracing::debug!(node_id = %node.id, error = %err, "memory extraction skipped");
            }
        });
    }
}
