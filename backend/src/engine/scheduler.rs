//! The run scheduler.
//!
//! A run is created `queued`; a dispatcher on some backend instance claims it
//! (`FOR UPDATE SKIP LOCKED`) and executes its DAG: ready nodes are taken
//! from a priority queue ordered by critical-path length, at most
//! `max_concurrency` run at once (semaphore), each attempt has a timeout,
//! retryable failures back off with full jitter, descendants of failed nodes
//! are skipped, and unchanged nodes are served from the content-hash cache.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde_json::{Value, json};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::analysis::dependency_graph;
use super::defend::{About, Finding};
use super::executor::{self, ExecContext, ExecError, ExecOutput};
use super::policy as policies;
use super::trace::{self, Sealed, TraceHandle};
use super::{artifacts, credentials, editor, guardrails, knowledge, usage};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::agent::{Agent, AgentStatus};
use crate::domain::alert::AlertKind;
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
use crate::domain::settings::{ConfigScope, LlmProviderKind};
use crate::domain::trace::{SYSTEM_PRINCIPAL, SpanDraft, SpanKind, SpanStatus};
use crate::domain::usage::{UsageEvent, UsagePurpose};
use crate::dsa::priority::ReadyQueue;
use crate::llm::LlmTarget;
use crate::llm::pricing::{cost_usd, cost_usd_cached};
use crate::llm::retry::full_jitter;
use crate::realtime::events::{LogLevel, NodeStatusEvent, SseEvent};
use crate::repo::audit::Subject;
use crate::repo::runs::{NodeOutcome, RunRow};
use crate::repo::{self, OrNotFound};
use crate::{kernel, memory, orchestrator};

const DISPATCH_POLL: Duration = Duration::from_secs(2);
const CANCEL_POLL: Duration = Duration::from_secs(1);
const ORPHAN_AFTER_SECS: i64 = 60;

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
    let graph = editor::load_graph(state, owner, graph_id).await?;
    if graph.nodes.is_empty() {
        return Err(AppError::Unprocessable(
            "the graph has no nodes to run".into(),
        ));
    }
    let selected: Vec<&GraphNode> = match &opts.node_ids {
        None => graph.nodes.iter().collect(),
        Some(ids) => {
            let wanted: HashSet<Uuid> = ids.iter().copied().collect();
            let found: Vec<&GraphNode> = graph
                .nodes
                .iter()
                .filter(|n| wanted.contains(&n.id))
                .collect();
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

/// Claims and executes queued runs until `shutdown`.
pub async fn dispatcher(state: AppState, shutdown: CancellationToken) {
    loop {
        loop {
            match repo::runs::claim_next(&state.db, state.engine.instance()).await {
                Ok(Some(row)) => {
                    tokio::spawn(execute_run(state.clone(), row));
                }
                Ok(None) => break,
                Err(err) => {
                    tracing::error!(error = %err, "cannot claim runs");
                    break;
                }
            }
        }
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = state.engine.woken() => {}
            _ = tokio::time::sleep(DISPATCH_POLL) => {}
        }
    }
}

/// Fails runs whose executing instance died and publishes their final state.
pub async fn reap_orphans(state: &AppState) -> anyhow::Result<()> {
    repo::runs::heartbeat(&state.db, state.engine.instance()).await?;
    for run_id in repo::runs::fail_orphans(&state.db, ORPHAN_AFTER_SECS).await? {
        for node_id in
            repo::runs::close_unfinished_nodes(&state.db, run_id, NodeStatus::Failed).await?
        {
            repo::nodes::set_status(&state.db, node_id, NodeStatus::Failed, None).await?;
        }
        let run = load_run(state, run_id).await?;
        tracing::warn!(%run_id, "run orphaned by a stopped instance marked failed");
        state
            .hub
            .publish(run.graph_id, SseEvent::RunFinished { run });
    }
    Ok(())
}

async fn execute_run(state: AppState, row: RunRow) {
    execute_claimed(state, row, Vec::new(), None).await;
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
) {
    let token = state.engine.register_run(row.id);
    let started = Instant::now();
    if let Ok(run) = load_run(&state, row.id).await {
        state
            .hub
            .publish(row.graph_id, SseEvent::RunStarted { run });
    }
    // Every span of the run goes through one chain, in order of receipt.
    let sink = trace::start(state.clone(), row.id, row.graph_id);
    for span in opening {
        sink.handle().emit(span);
    }
    let statuses = match refusal {
        Some(reason) => {
            fail_all_nodes(&state, &row, &reason).await;
            vec![NodeStatus::Failed]
        }
        None => match Execution::prepare(&state, &row, token.clone(), sink.handle()).await {
            Ok(execution) => execution.drive().await,
            Err(err) => {
                tracing::error!(run_id = %row.id, error = %err, "run preparation failed");
                Vec::new()
            }
        },
    };
    let cancelled = token.is_cancelled();
    let status = if statuses.is_empty() {
        RunStatus::Failed
    } else {
        final_status(&statuses, cancelled)
    };
    let sealed = sink.seal().await;
    if let Err(err) = finish_run(&state, row.id, status, cancelled, sealed).await {
        tracing::error!(run_id = %row.id, error = %err, "cannot finish run");
    }
    state.metrics.runs.add(&[("status", status.as_str())], 1);
    state.metrics.run_duration.observe(started.elapsed());
    state.engine.unregister_run(row.id);
}

/// Fails every node of a run before any is started, with `reason` (a task
/// the firewall refused).
async fn fail_all_nodes(state: &AppState, row: &RunRow, reason: &str) {
    let nodes =
        match repo::runs::close_unfinished_nodes(&state.db, row.id, NodeStatus::Failed).await {
            Ok(nodes) => nodes,
            Err(err) => {
                tracing::error!(run_id = %row.id, error = %err, "refused run's nodes not closed");
                return;
            }
        };
    for node_id in nodes {
        if let Err(err) = repo::runs::set_node_error(&state.db, row.id, node_id, reason).await {
            tracing::error!(run_id = %row.id, error = %err, "refused run's node reason not recorded");
        }
        if let Err(err) =
            repo::nodes::set_status(&state.db, node_id, NodeStatus::Failed, None).await
        {
            tracing::error!(run_id = %row.id, error = %err, "refused run's node status not recorded");
        }
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

/// Closes the run: leftover nodes, the final status, the seal of its trace
/// (on the run and in the audit log), and `run.finished`.
async fn finish_run(
    state: &AppState,
    run_id: Uuid,
    status: RunStatus,
    cancelled: bool,
    sealed: Sealed,
) -> anyhow::Result<()> {
    let leftover = if cancelled {
        NodeStatus::Cancelled
    } else {
        NodeStatus::Skipped
    };
    for node_id in repo::runs::close_unfinished_nodes(&state.db, run_id, leftover).await? {
        repo::nodes::set_status(&state.db, node_id, leftover, None).await?;
    }
    let hash = sealed.hash;
    let row = repo::runs::finish(
        &state.db,
        run_id,
        status,
        hash.as_ref().map(|h| h.as_slice()),
    )
    .await?;
    if let Some(hash) = hash {
        seal_audit(state, &row, hash, sealed.spans).await;
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

/// Writes the final hash of a run's trace chain to its workspace's audit
/// log (`trace_sealed`, subject: the run id). A graph outside any workspace
/// has no audit log; its seal lives on the run alone.
pub async fn seal_audit(state: &AppState, row: &RunRow, hash: [u8; 32], spans: i64) {
    let workspace = match repo::graphs::workspace_of(&state.db, row.graph_id).await {
        Ok(workspace) => workspace,
        Err(err) => {
            tracing::error!(run_id = %row.id, error = %err, "cannot find the run's workspace");
            None
        }
    };
    let Some(workspace) = workspace else {
        return;
    };
    let detail = format!("{spans} span(s), sha256 {}", hex::encode(hash));
    if let Err(err) = repo::audit::record(
        &state.db,
        workspace,
        row.owner_id,
        AuditAction::TraceSealed,
        Subject::Text(&row.id.to_string()),
        &detail,
    )
    .await
    {
        tracing::error!(run_id = %row.id, error = %err, "trace seal was not written to the audit log");
    }
}

/// Shared, immutable data of one run execution.
struct RunData {
    run_id: Uuid,
    graph_id: Uuid,
    owner: Uuid,
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
    ontology: Ontology,
    edges: Vec<GraphEdge>,
    /// The run's trace chain.
    trace: TraceHandle,
    /// The agent every node runs as, when the graph is an exposed agent's
    /// A2A graph (its tasks are its own, whatever role the node names).
    pinned_agent: Option<Uuid>,
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

impl Execution {
    async fn prepare(
        state: &AppState,
        row: &RunRow,
        token: CancellationToken,
        trace: TraceHandle,
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
        // The workspace level governs the whole run; each node records every level.
        let versions = policies::versions_of(&policies::levels_for(&policies, None, None));
        repo::runs::set_policy_versions(&state.db, row.id, &versions).await?;
        let pinned_agent = repo::a2a::pinned_agent_of_graph(&state.db, row.graph_id).await?;
        let data = RunData {
            run_id: row.id,
            graph_id: row.graph_id,
            owner: row.owner_id,
            goal: graph.goal,
            force: row.force,
            target,
            credential,
            policy: guardrails::load(state, graph.workspace_id).await?,
            policies,
            workspace_id: graph.workspace_id,
            ontology: graph.ontology,
            edges,
            trace,
            pinned_agent,
        };
        Ok(Execution {
            state: state.clone(),
            data: Arc::new(data),
            token,
            nodes,
            max_concurrency: row.max_concurrency.max(1) as usize,
        })
    }

    /// Executes the DAG; returns the final status of every node.
    async fn drive(self) -> Vec<NodeStatus> {
        let (_, dag) = dependency_graph(&self.nodes, &self.data.edges);
        let priority = dag.remaining_depth();
        let mut indegree: Vec<usize> = (0..dag.len()).map(|v| dag.predecessors(v).len()).collect();
        let mut statuses = vec![NodeStatus::Queued; dag.len()];
        let mut outputs = self.initial_outputs().await;
        let mut ready = ReadyQueue::default();
        for v in (0..dag.len()).filter(|&v| indegree[v] == 0) {
            ready.push(priority[v], v);
        }
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
        let watcher =
            spawn_cancel_watcher(self.state.clone(), self.data.run_id, self.token.clone());
        let semaphore = Arc::new(Semaphore::new(self.max_concurrency));
        let mut tasks = JoinSet::new();
        loop {
            while !self.token.is_cancelled()
                && let Some(v) = ready.pop()
            {
                statuses[v] = NodeStatus::Running;
                tasks.spawn(run_node(
                    self.state.clone(),
                    self.data.clone(),
                    self.token.clone(),
                    semaphore.clone(),
                    v,
                    self.nodes[v].clone(),
                    upstream_of(&self.nodes[v], &self.data.edges, &outputs),
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
        watcher.cancel();
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

fn spawn_cancel_watcher(
    state: AppState,
    run_id: Uuid,
    token: CancellationToken,
) -> CancellationToken {
    let done = CancellationToken::new();
    let stop = done.clone();
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = stop.cancelled() => return,
                _ = token.cancelled() => return,
                _ = tokio::time::sleep(CANCEL_POLL) => {}
            }
            if let Ok(true) = repo::runs::cancel_requested(&state.db, run_id).await {
                token.cancel();
                return;
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
    if let Err(err) = record(state, data, node_id, &outcome, None, 0, None).await {
        tracing::error!(%node_id, error = %err, "cannot record skipped node");
    }
    publish_status(
        state,
        data,
        node_id,
        NodeStatus::Skipped,
        0,
        Some(error.into()),
        false,
    );
}

/// Persists a node outcome (node run, run totals, agent spend, node status).
async fn record(
    state: &AppState,
    data: &RunData,
    node_id: Uuid,
    outcome: &NodeOutcome<'_>,
    agent: Option<&Agent>,
    context_chars_saved: i64,
    span_id: Option<Uuid>,
) -> anyhow::Result<()> {
    let mut tx = state.db.begin().await?;
    repo::runs::finish_node(&mut tx, data.run_id, node_id, outcome).await?;
    if let Some(agent) = agent
        && !outcome.cached
    {
        repo::agents::add_spent(&mut *tx, agent.id, outcome.tokens_in + outcome.tokens_out).await?;
    }
    repo::nodes::set_status(&mut *tx, node_id, outcome.status, outcome.output).await?;
    tx.commit().await?;
    if let Some(agent) = agent
        && !outcome.cached
    {
        // The card shows what the agent spent.
        state
            .caches
            .invalidate(crate::app::caches::Key::Agent(agent.id));
    }
    if !outcome.cached {
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
        usage::record(state, event).await;
    }
    state
        .metrics
        .node_runs
        .add(&[("status", outcome.status.as_str())], 1);
    Ok(())
}

/// Content hash used for memoization: everything that influences the result.
fn content_hash(data: &RunData, node: &GraphNode, upstream: &[(Uuid, String)]) -> String {
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
        "type": data.ontology.node_type(&node.kind),
        "executor": node.executor,
        "agent_role": node.agent_role,
        "provider": data.target.provider,
        "model": data.target.model,
        "upstream": upstream,
    });
    kernel::sha256_hex(material.to_string().as_bytes())
}

async fn run_node(
    state: AppState,
    data: Arc<RunData>,
    token: CancellationToken,
    semaphore: Arc<Semaphore>,
    index: usize,
    node: GraphNode,
    upstream: Vec<(Uuid, String)>,
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
    match NodeTask::new(state.clone(), data.clone(), token, node, upstream).await {
        Ok(task) => task.run(index).await,
        Err(err) => {
            let message = err.to_string();
            let outcome = NodeOutcome {
                status: NodeStatus::Failed,
                attempt: 0,
                tokens_in: 0,
                tokens_out: 0,
                cost_usd: 0.0,
                cached: false,
                error: Some(&message),
                output: None,
                content_hash: None,
                evidence: None,
            };
            if let Err(e) = record(&state, &data, node_id, &outcome, None, 0, None).await {
                tracing::error!(%node_id, error = %e, "cannot record node failure");
            }
            publish_status(
                &state,
                &data,
                node_id,
                NodeStatus::Failed,
                0,
                Some(message),
                false,
            );
            NodeResult {
                index,
                status: NodeStatus::Failed,
                output: None,
            }
        }
    }
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
    ) -> anyhow::Result<Self> {
        let began = Utc::now();
        let hash = content_hash(&data, &node, &upstream);
        let agent = match (data.pinned_agent, data.workspace_id) {
            (Some(pinned), Some(workspace)) => {
                repo::agents::find(&state.db, workspace, pinned).await?
            }
            _ => orchestrator::assign(&state, data.workspace_id, &node, &data.ontology).await?,
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
                    .and_then(|e| data.ontology.relation_type(&e.kind))
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
            }),
        });
        let ctx = ExecContext {
            state,
            run_id: data.run_id,
            graph_id: data.graph_id,
            workspace_id: data.workspace_id,
            owner: data.owner,
            goal: data.goal.clone(),
            node_type: data.ontology.node_type(&node.kind).cloned(),
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
        };
        Ok(NodeTask {
            ctx,
            data,
            token,
            hash,
            context_saved,
        })
    }

    async fn run(self, index: usize) -> NodeResult {
        let status_and_output = match self.serve_cached().await {
            Some(output) => (NodeStatus::Succeeded, Some(output)),
            None => self.attempts().await,
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
        record(state, &self.data, node_id, &outcome, None, 0, None)
            .await
            .ok()?;
        if let Err(err) = artifacts::copy_from_run(
            state,
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

    async fn attempts(&self) -> (NodeStatus, Option<String>) {
        if let Some(reason) = self.budget_error() {
            return self.finish(NodeStatus::Failed, 0, None, Some(reason)).await;
        }
        // The model allow-list is enforced here, before anything is dispatched.
        let model = self.ctx.agent_target().model;
        let decision = self.ctx.decide(&Action::Model(&model));
        if !decision.allowed {
            self.ctx
                .policy_span(&model, &decision, json!({"action": "model"}));
            let reason = format!("policy: {}", decision.reason);
            return self.finish(NodeStatus::Failed, 0, None, Some(reason)).await;
        }
        // So are the rules only the backend can see: working hours, the day's spend, the pace.
        if let Some(decision) = self.gate().await {
            self.ctx
                .policy_span(decision.rule, &decision, json!({"action": "node"}));
            let reason = format!("policy: {}", decision.reason);
            return self.finish(NodeStatus::Failed, 0, None, Some(reason)).await;
        }
        let state = &self.ctx.state;
        let settings = state.settings.clone();
        let executor = executor::for_kind(self.ctx.node.executor);
        let agent_id = self.ctx.agent.as_ref().map(|a| a.id);
        let node_id = self.ctx.node.id;
        let max_attempts = settings.max_attempts as i32;
        for attempt in 1..=max_attempts {
            if let Err(err) = self.mark_running(attempt, agent_id).await {
                return self
                    .finish(NodeStatus::Failed, attempt, None, Some(err.to_string()))
                    .await;
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

    async fn mark_running(&self, attempt: i32, agent_id: Option<Uuid>) -> anyhow::Result<()> {
        let state = &self.ctx.state;
        let node_id = self.ctx.node.id;
        self.ctx
            .attempt
            .store(attempt, std::sync::atomic::Ordering::Relaxed);
        repo::runs::start_node(
            &state.db,
            self.data.run_id,
            node_id,
            attempt,
            agent_id,
            &self.ctx.rules.versions,
            &self.ctx.labels_given,
        )
        .await?;
        repo::nodes::set_status(&state.db, node_id, NodeStatus::Running, None).await?;
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
        if let Err(err) = record(
            state,
            &self.data,
            node_id,
            &outcome,
            self.ctx.agent.as_ref(),
            self.context_saved,
            out.as_ref().and_then(|o| o.span_id),
        )
        .await
        {
            tracing::error!(%node_id, error = %err, "cannot record node outcome");
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
