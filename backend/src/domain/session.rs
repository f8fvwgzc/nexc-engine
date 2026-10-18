//! Sessions: one run seen as a tree. A run has nodes; a node was run by an
//! agent; the agent spawned sub-agents (each a `spawn` span); every agent
//! made model turns and tool calls. The tree is computed from the run's
//! spans and node results, with tokens, cost, calls and denials rolled up
//! at every level, so the console can show where a run's budget went and
//! stop one subtree while the rest goes on.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use utoipa::ToSchema;
use uuid::Uuid;

use super::graph::{Executor, NodeStatus};
use super::run::RunStatus;
use super::string_enum;
use super::trace::{Span, SpanKind, SpanStatus};

/// The tool a parent calls to spawn a sub-agent; its `tool_call` span ends
/// when the child does.
pub const SPAWN_TOOL: &str = "spawn_subagent";
/// `attributes.action` of the `policy` span an operator's cancel records.
pub const CANCEL_ACTION: &str = "cancel_agent";
/// `attributes.decision` of that span.
pub const CANCEL_DECISION: &str = "cancelled";

string_enum!(
    /// Where a node or an agent subtree stands.
    SessionStatus {
        Queued => "queued",
        Running => "running",
        /// A tool call of the subtree waits for a human.
        WaitingApproval => "waiting_approval",
        Succeeded => "succeeded",
        Failed => "failed",
        Cancelled => "cancelled",
        /// A node that never ran because an upstream node failed.
        Skipped => "skipped",
    }
);

impl SessionStatus {
    /// True once the subtree can no longer change.
    pub fn is_terminal(self) -> bool {
        !matches!(
            self,
            SessionStatus::Queued | SessionStatus::Running | SessionStatus::WaitingApproval
        )
    }
}

impl From<NodeStatus> for SessionStatus {
    fn from(status: NodeStatus) -> Self {
        match status {
            NodeStatus::Idle | NodeStatus::Queued => SessionStatus::Queued,
            NodeStatus::Running => SessionStatus::Running,
            NodeStatus::WaitingApproval => SessionStatus::WaitingApproval,
            NodeStatus::Succeeded => SessionStatus::Succeeded,
            NodeStatus::Failed => SessionStatus::Failed,
            NodeStatus::Skipped => SessionStatus::Skipped,
            NodeStatus::Cancelled => SessionStatus::Cancelled,
        }
    }
}

/// What a subtree spent and did.
#[derive(Debug, Clone, Default, PartialEq, Serialize, ToSchema)]
pub struct Rollup {
    pub spans: i64,
    /// Model turns.
    pub llm_calls: i64,
    pub tool_calls: i64,
    /// Policy spans whose decision was `deny`.
    pub denials: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    /// Of `tokens_in`, the ones read from the provider's prompt cache.
    pub tokens_cached: i64,
    pub cost_usd: f64,
}

impl Rollup {
    fn add(&mut self, other: &Rollup) {
        self.spans += other.spans;
        self.llm_calls += other.llm_calls;
        self.tool_calls += other.tool_calls;
        self.denials += other.denials;
        self.tokens_in += other.tokens_in;
        self.tokens_out += other.tokens_out;
        self.tokens_cached += other.tokens_cached;
        self.cost_usd += other.cost_usd;
    }

    fn scale_cost(&mut self, factor: f64) {
        self.cost_usd *= factor;
    }
}

/// Usage of one model across a run.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ModelRollup {
    pub provider: String,
    pub model: String,
    pub llm_calls: i64,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub tokens_cached: i64,
    pub cost_usd: f64,
}

/// One agent of a node: the agent the node ran as (depth 0) or a sub-agent
/// it spawned, with its own turns and tool calls and what hangs under it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SessionAgent {
    /// `agent://{workspace}/{name}`, sub-agents `…/{role}-{n}`; `system` for a node without an agent.
    pub principal: String,
    #[schema(required = true)]
    pub agent_id: Option<Uuid>,
    /// The role a sub-agent was spawned as.
    #[schema(required = true)]
    pub role: Option<String>,
    /// 0 for the agent the node ran as.
    pub depth: i32,
    /// The `spawn` span a sub-agent was born in (what `POST /runs/{id}/agents/{span_id}/cancel` names); `null` at depth 0.
    #[schema(required = true)]
    pub spawn_span_id: Option<Uuid>,
    #[schema(required = true)]
    pub parent_principal: Option<String>,
    pub status: SessionStatus,
    #[schema(required = true)]
    pub started_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub ended_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub duration_ms: Option<i64>,
    /// The agent's own turns and tool calls.
    pub own: Rollup,
    /// Its own plus every sub-agent under it.
    pub total: Rollup,
    /// Its own spans in chain order: turns, tool calls, policy decisions, approvals.
    pub span_ids: Vec<Uuid>,
    /// Whether an operator may stop it now: a sub-agent still working in a node still running.
    pub cancellable: bool,
    /// The sub-agents it spawned, in birth order.
    #[schema(no_recursion)]
    pub children: Vec<SessionAgent>,
}

/// One node of the run.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SessionNode {
    pub node_id: Uuid,
    pub title: String,
    pub executor: Executor,
    pub status: SessionStatus,
    pub attempt: i32,
    /// Served from the result cache: no agent worked.
    pub cached: bool,
    #[schema(required = true)]
    pub error: Option<String>,
    #[schema(required = true)]
    pub started_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub ended_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub duration_ms: Option<i64>,
    pub rollup: Rollup,
    /// What the node was given before it ran (`data_read`) and what was withheld (`policy` named `context`).
    pub context_span_ids: Vec<Uuid>,
    /// The agent the node ran as, with its sub-agents under it.
    pub agents: Vec<SessionAgent>,
}

/// A run as a tree.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Session {
    pub run_id: Uuid,
    pub graph_id: Uuid,
    pub graph_name: String,
    #[schema(required = true)]
    pub workspace_id: Option<Uuid>,
    pub status: RunStatus,
    pub created_at: DateTime<Utc>,
    #[schema(required = true)]
    pub started_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub finished_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub duration_ms: Option<i64>,
    pub rollup: Rollup,
    /// Usage by model, most costly first.
    pub by_model: Vec<ModelRollup>,
    /// Whether prompts and completions were being recorded when the run's nodes started.
    pub recording: bool,
    pub nodes: Vec<SessionNode>,
}

/// An agent that worked in a run, as the list names it.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SessionAgentRef {
    pub id: Uuid,
    pub name: String,
}

/// One row of a workspace's session list: a run with its rollups.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SessionSummary {
    pub run_id: Uuid,
    pub graph_id: Uuid,
    pub graph_name: String,
    pub status: RunStatus,
    pub created_at: DateTime<Utc>,
    #[schema(required = true)]
    pub started_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub finished_at: Option<DateTime<Utc>>,
    #[schema(required = true)]
    pub duration_ms: Option<i64>,
    pub nodes: i64,
    /// The workspace agents that ran its nodes.
    pub agents: Vec<SessionAgentRef>,
    pub rollup: Rollup,
}

/// A page of a workspace's sessions, newest first.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct SessionPage {
    pub sessions: Vec<SessionSummary>,
    /// Pass as `before` for the next page; `null` on the last.
    #[schema(required = true)]
    pub next: Option<Uuid>,
}

/// The run, as the tree is built from.
#[derive(Debug, Clone)]
pub struct RunFacts {
    pub id: Uuid,
    pub graph_id: Uuid,
    pub graph_name: String,
    pub workspace_id: Option<Uuid>,
    pub status: RunStatus,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// One node result, as the tree is built from.
#[derive(Debug, Clone)]
pub struct NodeFacts {
    pub node_id: Uuid,
    pub title: String,
    pub executor: Executor,
    pub status: NodeStatus,
    pub attempt: i32,
    pub cached: bool,
    pub error: Option<String>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub tokens_in: i64,
    pub tokens_out: i64,
    pub cost_usd: f64,
}

/// `(model, tokens in, tokens out, cached tokens) -> dollars`: what one model turn cost.
pub type Pricing<'a> = &'a dyn Fn(&str, i64, i64, i64) -> f64;

fn text<'a>(span: &'a Span, key: &str) -> Option<&'a str> {
    span.attributes.get(key).and_then(Value::as_str)
}

fn number(span: &Span, key: &str) -> i64 {
    span.attributes
        .get(key)
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0)
}

fn millis(from: Option<DateTime<Utc>>, to: Option<DateTime<Utc>>) -> Option<i64> {
    Some((to? - from?).num_milliseconds().max(0))
}

/// Whether `span` is about the node's context rather than an agent's work.
fn is_context(span: &Span) -> bool {
    span.kind == SpanKind::DataRead
        || (span.kind == SpanKind::Policy
            && span.name == "context"
            && text(span, "action") == Some("data_read"))
}

/// What one span adds to its agent's rollup.
fn rollup_of(span: &Span, price: Pricing<'_>) -> Rollup {
    let mut r = Rollup {
        spans: 1,
        ..Rollup::default()
    };
    match span.kind {
        SpanKind::LlmCall => {
            r.llm_calls = 1;
            r.tokens_in = number(span, "tokens_in");
            r.tokens_out = number(span, "tokens_out");
            r.tokens_cached = number(span, "tokens_cached");
            r.cost_usd = if text(span, "provider") == Some("demo") {
                0.0
            } else {
                let model = text(span, "model").unwrap_or(&span.name);
                price(model, r.tokens_in, r.tokens_out, r.tokens_cached)
            };
        }
        SpanKind::ToolCall => r.tool_calls = 1,
        SpanKind::Policy if text(span, "decision") == Some("deny") => r.denials = 1,
        _ => {}
    }
    r
}

/// An agent while the tree is built.
struct Building<'a> {
    principal: String,
    agent_id: Option<Uuid>,
    role: Option<String>,
    depth: i32,
    spawn: Option<&'a Span>,
    parent: Option<usize>,
    spans: Vec<&'a Span>,
    children: Vec<usize>,
}

impl<'a> Building<'a> {
    fn new(principal: &str, agent_id: Option<Uuid>) -> Self {
        Building {
            principal: principal.to_owned(),
            agent_id,
            role: None,
            depth: 0,
            spawn: None,
            parent: None,
            spans: Vec::new(),
            children: Vec::new(),
        }
    }
}

/// Builds the agents of one node from its spans: the root agent, a child per
/// `spawn` span, every other span under the agent whose principal it names.
fn agents_of<'a>(spans: &[&'a Span], root_principal: &str) -> Vec<Building<'a>> {
    let root_id = spans.iter().find_map(|s| s.agent_id);
    let mut agents = vec![Building::new(root_principal, root_id)];
    let mut index: HashMap<String, usize> = HashMap::from([(root_principal.to_owned(), 0)]);
    for span in spans {
        if span.kind == SpanKind::Spawn {
            let child = text(span, "child").unwrap_or(&span.name).to_owned();
            if index.contains_key(&child) {
                continue;
            }
            let parent = index.get(&span.principal).copied().unwrap_or(0);
            let depth = span
                .attributes
                .get("depth")
                .and_then(Value::as_i64)
                .map_or(agents[parent].depth + 1, |d| d as i32);
            let at = agents.len();
            agents.push(Building {
                principal: child.clone(),
                agent_id: span.agent_id.or(agents[parent].agent_id),
                role: text(span, "role").map(str::to_owned),
                depth,
                spawn: Some(span),
                parent: Some(parent),
                spans: Vec::new(),
                children: Vec::new(),
            });
            agents[parent].children.push(at);
            index.insert(child, at);
        }
    }
    // Every span goes under the agent whose principal it names; an unknown principal under
    // the closest ancestor by name, else the root.
    for span in spans {
        let owner = match index.get(&span.principal) {
            Some(at) => *at,
            None => index
                .iter()
                .filter(|(principal, _)| {
                    span.principal
                        .strip_prefix(principal.as_str())
                        .is_some_and(|rest| rest.starts_with('/'))
                })
                .max_by_key(|(principal, _)| principal.len())
                .map_or(0, |(_, at)| *at),
        };
        agents[owner].spans.push(span);
    }
    agents
}

/// The `spawn_subagent` call of the parent that ended when the child did:
/// the n-th such call of the turn the n-th spawn of that turn belongs to.
fn spawn_call<'a>(parent: &Building<'a>, spawn: &Span) -> Option<&'a Span> {
    let siblings: Vec<&&Span> = parent
        .spans
        .iter()
        .filter(|s| s.kind == SpanKind::Spawn && s.parent_id == spawn.parent_id)
        .collect();
    let position = siblings.iter().position(|s| s.id == spawn.id)?;
    parent
        .spans
        .iter()
        .filter(|s| {
            s.kind == SpanKind::ToolCall && s.name == SPAWN_TOOL && s.parent_id == spawn.parent_id
        })
        .nth(position)
        .copied()
}

/// Whether an operator stopped the sub-agent born in `spawn`: a `policy`
/// span of action `cancel_agent` naming the spawn.
fn cancelled(spans: &[&Span], spawn: &Span) -> bool {
    spans.iter().any(|s| {
        s.kind == SpanKind::Policy
            && text(s, "action") == Some(CANCEL_ACTION)
            && text(s, "spawn_span_id") == Some(spawn.id.to_string().as_str())
    })
}

/// Whether a tool call of the agent still waits for a human: a request
/// without its decision.
fn waiting(agent: &Building<'_>) -> bool {
    let decided: Vec<&str> = agent
        .spans
        .iter()
        .filter(|s| s.kind == SpanKind::Approval)
        .filter_map(|s| text(s, "approval_id"))
        .collect();
    agent.spans.iter().any(|s| {
        s.kind == SpanKind::Policy
            && text(s, "decision") == Some("require_approval")
            && text(s, "approval_id").is_some_and(|id| !decided.contains(&id))
    })
}

fn finish_agent(
    agents: &[Building<'_>],
    at: usize,
    node: &NodeFacts,
    node_status: SessionStatus,
    all: &[&Span],
    price: Pricing<'_>,
) -> SessionAgent {
    let agent = &agents[at];
    let own = agent.spans.iter().fold(Rollup::default(), |mut acc, s| {
        acc.add(&rollup_of(s, price));
        acc
    });
    let children: Vec<SessionAgent> = agent
        .children
        .iter()
        .map(|c| finish_agent(agents, *c, node, node_status, all, price))
        .collect();
    let mut total = own.clone();
    for child in &children {
        total.add(&child.total);
    }
    let (status, started_at, ended_at) = match (agent.spawn, agent.parent) {
        (Some(spawn), Some(parent)) => {
            let call = spawn_call(&agents[parent], spawn);
            let stopped = cancelled(all, spawn);
            let status = match call {
                Some(_) if stopped => SessionStatus::Cancelled,
                Some(call) if call.status == SpanStatus::Error => SessionStatus::Failed,
                Some(_) => SessionStatus::Succeeded,
                None if stopped => SessionStatus::Cancelled,
                None if node_status.is_terminal() => match node_status {
                    SessionStatus::Cancelled => SessionStatus::Cancelled,
                    _ => SessionStatus::Failed,
                },
                None if waiting(agent) => SessionStatus::WaitingApproval,
                None => SessionStatus::Running,
            };
            let ended = call.and_then(|c| c.ended_at).or_else(|| {
                status
                    .is_terminal()
                    .then(|| {
                        agent
                            .spans
                            .iter()
                            .filter_map(|s| s.ended_at)
                            .max()
                            .or(node.finished_at)
                    })
                    .flatten()
            });
            (status, Some(spawn.started_at), ended)
        }
        _ => {
            let status = if node_status == SessionStatus::Running && waiting(agent) {
                SessionStatus::WaitingApproval
            } else {
                node_status
            };
            let started = node
                .started_at
                .or_else(|| agent.spans.first().map(|s| s.started_at));
            (status, started, node.finished_at)
        }
    };
    SessionAgent {
        principal: agent.principal.clone(),
        agent_id: agent.agent_id,
        role: agent.role.clone(),
        depth: agent.depth,
        spawn_span_id: agent.spawn.map(|s| s.id),
        parent_principal: agent.parent.map(|p| agents[p].principal.clone()),
        status,
        started_at,
        ended_at,
        duration_ms: millis(started_at, ended_at),
        own,
        total,
        span_ids: agent.spans.iter().map(|s| s.id).collect(),
        cancellable: agent.spawn.is_some()
            && !status.is_terminal()
            && !node_status.is_terminal()
            && node.executor == Executor::Agent,
        children,
    }
}

fn scale_agent(agent: &mut SessionAgent, factor: f64) {
    agent.own.scale_cost(factor);
    agent.total.scale_cost(factor);
    for child in &mut agent.children {
        scale_agent(child, factor);
    }
}

/// The tree of one node.
fn build_node(node: &NodeFacts, spans: &[&Span], price: Pricing<'_>) -> SessionNode {
    let node_status = SessionStatus::from(node.status);
    let context: Vec<&Span> = spans.iter().copied().filter(|s| is_context(s)).collect();
    let work: Vec<&Span> = spans.iter().copied().filter(|s| !is_context(s)).collect();
    let root_principal = context
        .first()
        .map(|s| s.principal.as_str())
        .or_else(|| work.first().map(|s| s.principal.as_str()))
        .unwrap_or(super::trace::SYSTEM_PRINCIPAL);
    let building = agents_of(&work, root_principal);
    let mut agents = if work.is_empty() {
        Vec::new()
    } else {
        vec![finish_agent(&building, 0, node, node_status, &work, price)]
    };
    let mut rollup = agents.iter().fold(Rollup::default(), |mut acc, a| {
        acc.add(&a.total);
        acc
    });
    // The context spans count on the node, not on an agent.
    rollup.spans += context.len() as i64;
    rollup.denials += context
        .iter()
        .filter(|s| s.kind == SpanKind::Policy && text(s, "decision") == Some("deny"))
        .count() as i64;
    // What the node result recorded wins where the spans say less: a node served from the
    // cache has tokens and no spans, a finished node has its cost on the record.
    rollup.tokens_in = rollup.tokens_in.max(node.tokens_in);
    rollup.tokens_out = rollup.tokens_out.max(node.tokens_out);
    if node_status.is_terminal() && node.cost_usd > 0.0 && !node.cached {
        if rollup.cost_usd > 0.0 {
            let factor = node.cost_usd / rollup.cost_usd;
            for agent in &mut agents {
                scale_agent(agent, factor);
            }
        }
        rollup.cost_usd = node.cost_usd;
    }
    SessionNode {
        node_id: node.node_id,
        title: node.title.clone(),
        executor: node.executor,
        status: node_status,
        attempt: node.attempt,
        cached: node.cached,
        error: node.error.clone(),
        started_at: node.started_at,
        ended_at: node.finished_at,
        duration_ms: millis(node.started_at, node.finished_at),
        rollup,
        context_span_ids: context.iter().map(|s| s.id).collect(),
        agents,
    }
}

/// Builds the session of a run from its node results and its spans (in
/// chain order). `price` says what a model turn cost.
pub fn build(run: RunFacts, nodes: &[NodeFacts], spans: &[Span], price: Pricing<'_>) -> Session {
    let mut by_node: HashMap<Uuid, Vec<&Span>> = HashMap::new();
    for span in spans {
        by_node.entry(span.node_id).or_default().push(span);
    }
    let mut out_nodes: Vec<SessionNode> = nodes
        .iter()
        .map(|node| {
            build_node(
                node,
                by_node.get(&node.node_id).map_or(&[], Vec::as_slice),
                price,
            )
        })
        .collect();
    // Spans of a node the run's results do not list (a node removed since) still count.
    let listed: Vec<Uuid> = nodes.iter().map(|n| n.node_id).collect();
    for (node_id, spans) in &by_node {
        if !listed.contains(node_id) {
            let facts = NodeFacts {
                node_id: *node_id,
                title: String::new(),
                executor: Executor::Agent,
                status: NodeStatus::Succeeded,
                attempt: 0,
                cached: false,
                error: None,
                started_at: spans.first().map(|s| s.started_at),
                finished_at: spans.iter().filter_map(|s| s.ended_at).max(),
                tokens_in: 0,
                tokens_out: 0,
                cost_usd: 0.0,
            };
            out_nodes.push(build_node(&facts, spans, price));
        }
    }
    let mut rollup = Rollup::default();
    for node in &out_nodes {
        rollup.add(&node.rollup);
    }
    let mut models: Vec<ModelRollup> = Vec::new();
    for span in spans.iter().filter(|s| s.kind == SpanKind::LlmCall) {
        let part = rollup_of(span, price);
        let provider = text(span, "provider").unwrap_or("").to_owned();
        let model = text(span, "model").unwrap_or(&span.name).to_owned();
        let entry = match models
            .iter_mut()
            .find(|m| m.provider == provider && m.model == model)
        {
            Some(entry) => entry,
            None => {
                models.push(ModelRollup {
                    provider,
                    model,
                    llm_calls: 0,
                    tokens_in: 0,
                    tokens_out: 0,
                    tokens_cached: 0,
                    cost_usd: 0.0,
                });
                models.last_mut().expect("just pushed")
            }
        };
        entry.llm_calls += 1;
        entry.tokens_in += part.tokens_in;
        entry.tokens_out += part.tokens_out;
        entry.tokens_cached += part.tokens_cached;
        entry.cost_usd += part.cost_usd;
    }
    models.sort_by(|a, b| {
        b.cost_usd
            .total_cmp(&a.cost_usd)
            .then(b.tokens_in.cmp(&a.tokens_in))
            .then(a.model.cmp(&b.model))
    });
    let recording = spans.iter().any(|s| {
        s.kind == SpanKind::DataRead
            && s.attributes.get("recording").and_then(Value::as_bool) == Some(true)
    });
    Session {
        run_id: run.id,
        graph_id: run.graph_id,
        graph_name: run.graph_name,
        workspace_id: run.workspace_id,
        status: run.status,
        created_at: run.created_at,
        started_at: run.started_at,
        finished_at: run.finished_at,
        duration_ms: millis(run.started_at, run.finished_at),
        rollup,
        by_model: models,
        recording,
        nodes: out_nodes,
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;
    use serde_json::json;

    use super::*;
    use crate::domain::trace::GENESIS;

    fn at(secs: i64) -> DateTime<Utc> {
        "2026-10-18T12:00:00Z".parse::<DateTime<Utc>>().unwrap() + Duration::seconds(secs)
    }

    #[allow(clippy::too_many_arguments)]
    fn span(
        seq: i32,
        node: Uuid,
        principal: &str,
        parent: Option<Uuid>,
        kind: SpanKind,
        name: &str,
        status: SpanStatus,
        attributes: Value,
    ) -> Span {
        Span {
            id: Uuid::now_v7(),
            run_id: Uuid::nil(),
            node_id: node,
            agent_id: None,
            principal: principal.into(),
            parent_id: parent,
            kind,
            name: name.into(),
            started_at: at(i64::from(seq)),
            ended_at: Some(at(i64::from(seq) + 1)),
            status,
            attributes,
            seq,
            prev_hash: GENESIS,
            hash: GENESIS,
        }
    }

    fn turn(seq: i32, node: Uuid, principal: &str, parent: Option<Uuid>, tokens: i64) -> Span {
        span(
            seq,
            node,
            principal,
            parent,
            SpanKind::LlmCall,
            "claude-test",
            SpanStatus::Ok,
            json!({"provider": "anthropic", "model": "claude-test", "tokens_in": tokens, "tokens_out": 10, "tokens_cached": 2}),
        )
    }

    fn run(status: RunStatus) -> RunFacts {
        RunFacts {
            id: Uuid::nil(),
            graph_id: Uuid::nil(),
            graph_name: "G".into(),
            workspace_id: None,
            status,
            created_at: at(0),
            started_at: Some(at(0)),
            finished_at: None,
        }
    }

    fn node(id: Uuid, status: NodeStatus) -> NodeFacts {
        NodeFacts {
            node_id: id,
            title: "Research".into(),
            executor: Executor::Agent,
            status,
            attempt: 1,
            cached: false,
            error: None,
            started_at: Some(at(0)),
            finished_at: SessionStatus::from(status).is_terminal().then(|| at(20)),
            tokens_in: 0,
            tokens_out: 0,
            cost_usd: 0.0,
        }
    }

    /// Every input token costs a dollar; a cached one a tenth.
    fn price(_: &str, tokens_in: i64, _: i64, cached: i64) -> f64 {
        (tokens_in - cached) as f64 + cached as f64 * 0.1
    }

    #[test]
    fn nested_spawns_roll_up_to_their_parents() {
        let n = Uuid::now_v7();
        let root = "agent://w/Analyst";
        let child = "agent://w/Analyst/researcher-1";
        let grandchild = "agent://w/Analyst/researcher-1/qa-1";
        let read = span(
            0,
            n,
            root,
            None,
            SpanKind::DataRead,
            "context",
            SpanStatus::Ok,
            json!({"recording": true}),
        );
        let t1 = turn(1, n, root, None, 100);
        let spawn = span(
            2,
            n,
            root,
            Some(t1.id),
            SpanKind::Spawn,
            child,
            SpanStatus::Ok,
            json!({"child": child, "role": "researcher", "depth": 1}),
        );
        let c1 = turn(3, n, child, Some(spawn.id), 50);
        let spawn2 = span(
            4,
            n,
            child,
            Some(c1.id),
            SpanKind::Spawn,
            grandchild,
            SpanStatus::Ok,
            json!({"child": grandchild, "role": "qa", "depth": 2}),
        );
        let g1 = turn(5, n, grandchild, Some(spawn2.id), 20);
        let g_tool = span(
            6,
            n,
            grandchild,
            Some(g1.id),
            SpanKind::ToolCall,
            "write_file",
            SpanStatus::Ok,
            json!({"tool": "write_file"}),
        );
        let g_call = span(
            7,
            n,
            child,
            Some(c1.id),
            SpanKind::ToolCall,
            SPAWN_TOOL,
            SpanStatus::Ok,
            json!({"tool": SPAWN_TOOL}),
        );
        let deny = span(
            8,
            n,
            child,
            Some(c1.id),
            SpanKind::Policy,
            "make_docx",
            SpanStatus::Error,
            json!({"decision": "deny", "rule": "tools.allow"}),
        );
        let c_call = span(
            9,
            n,
            root,
            Some(t1.id),
            SpanKind::ToolCall,
            SPAWN_TOOL,
            SpanStatus::Error,
            json!({"tool": SPAWN_TOOL}),
        );
        let t2 = turn(10, n, root, None, 30);
        let spans = vec![
            read, t1, spawn, c1, spawn2, g1, g_tool, g_call, deny, c_call, t2,
        ];
        let session = build(
            run(RunStatus::Succeeded),
            &[node(n, NodeStatus::Succeeded)],
            &spans,
            &price,
        );
        assert!(session.recording);
        let node = &session.nodes[0];
        assert_eq!(node.context_span_ids.len(), 1);
        let analyst = &node.agents[0];
        assert_eq!(analyst.principal, root);
        assert_eq!(analyst.depth, 0);
        assert_eq!(analyst.status, SessionStatus::Succeeded);
        assert_eq!(analyst.own.llm_calls, 2);
        assert_eq!(analyst.own.tool_calls, 1);
        assert_eq!(analyst.total.llm_calls, 4);
        assert_eq!(analyst.total.tool_calls, 3);
        assert_eq!(analyst.total.denials, 1);
        assert_eq!(analyst.total.tokens_in, 200);
        assert_eq!(analyst.total.tokens_cached, 8);
        let researcher = &analyst.children[0];
        assert_eq!(researcher.role.as_deref(), Some("researcher"));
        assert_eq!(researcher.depth, 1);
        assert_eq!(researcher.spawn_span_id, Some(spans[2].id));
        assert_eq!(researcher.parent_principal.as_deref(), Some(root));
        assert_eq!(
            researcher.status,
            SessionStatus::Failed,
            "its spawn call ended in error"
        );
        assert_eq!(researcher.own.denials, 1);
        assert_eq!(researcher.total.tokens_in, 70);
        let qa = &researcher.children[0];
        assert_eq!(qa.status, SessionStatus::Succeeded);
        assert_eq!(qa.total.tokens_in, 20);
        assert_eq!(qa.span_ids.len(), 2);
        assert_eq!(
            qa.ended_at, spans[7].ended_at,
            "ended when its spawn call did"
        );
        assert!(!qa.cancellable, "the node is over");
        assert_eq!(node.rollup.tokens_in, 200);
        assert_eq!(session.rollup.spans, 11);
        assert_eq!(session.by_model.len(), 1);
        assert_eq!(session.by_model[0].llm_calls, 4);
        assert!((session.by_model[0].cost_usd - (192.0 + 0.8)).abs() < 1e-9);
    }

    #[test]
    fn a_cancelled_subtree_and_a_waiting_approval_are_told_apart() {
        let n = Uuid::now_v7();
        let root = "agent://w/Analyst";
        let a = "agent://w/Analyst/researcher-1";
        let b = "agent://w/Analyst/writer-2";
        let t1 = turn(0, n, root, None, 10);
        let spawn_a = span(
            1,
            n,
            root,
            Some(t1.id),
            SpanKind::Spawn,
            a,
            SpanStatus::Ok,
            json!({"child": a, "role": "researcher", "depth": 1}),
        );
        let spawn_b = span(
            2,
            n,
            root,
            Some(t1.id),
            SpanKind::Spawn,
            b,
            SpanStatus::Ok,
            json!({"child": b, "role": "writer", "depth": 1}),
        );
        let a1 = turn(3, n, a, Some(spawn_a.id), 5);
        let cancel = span(
            4,
            n,
            a,
            None,
            SpanKind::Policy,
            a,
            SpanStatus::Error,
            json!({"decision": CANCEL_DECISION, "action": CANCEL_ACTION, "spawn_span_id": spawn_a.id}),
        );
        let b1 = turn(5, n, b, Some(spawn_b.id), 5);
        let ask = span(
            6,
            n,
            b,
            Some(b1.id),
            SpanKind::Policy,
            "make_docx",
            SpanStatus::Ok,
            json!({"decision": "require_approval", "approval_id": Uuid::now_v7()}),
        );
        let spans = vec![t1, spawn_a, spawn_b, a1, cancel, b1, ask];
        let session = build(
            run(RunStatus::Running),
            &[node(n, NodeStatus::WaitingApproval)],
            &spans,
            &price,
        );
        let analyst = &session.nodes[0].agents[0];
        assert_eq!(analyst.status, SessionStatus::WaitingApproval);
        let [researcher, writer] = analyst.children.as_slice() else {
            panic!("two children");
        };
        assert_eq!(researcher.status, SessionStatus::Cancelled);
        assert!(!researcher.cancellable);
        assert_eq!(researcher.own.denials, 0, "a cancel is not a denial");
        assert_eq!(writer.status, SessionStatus::WaitingApproval);
        assert!(writer.cancellable);
        assert_eq!(session.rollup.spans, 7);
        assert!(!session.recording);

        // Once the parent's spawn call for A comes back as an error, A stays cancelled.
        let mut with_call = spans.clone();
        with_call.push(span(
            7,
            n,
            root,
            Some(spans[0].id),
            SpanKind::ToolCall,
            SPAWN_TOOL,
            SpanStatus::Error,
            json!({"tool": SPAWN_TOOL}),
        ));
        let session = build(
            run(RunStatus::Running),
            &[node(n, NodeStatus::Running)],
            &with_call,
            &price,
        );
        let analyst = &session.nodes[0].agents[0];
        assert_eq!(analyst.status, SessionStatus::Running);
        assert_eq!(analyst.children[0].status, SessionStatus::Cancelled);
        assert_eq!(analyst.children[1].status, SessionStatus::WaitingApproval);
    }

    #[test]
    fn a_finished_node_keeps_its_recorded_cost_and_a_cached_one_its_tokens() {
        let n = Uuid::now_v7();
        let root = "agent://w/Analyst";
        let child = "agent://w/Analyst/qa-1";
        let t1 = turn(0, n, root, None, 100);
        let spawn = span(
            1,
            n,
            root,
            Some(t1.id),
            SpanKind::Spawn,
            child,
            SpanStatus::Ok,
            json!({"child": child, "depth": 1}),
        );
        let c1 = turn(2, n, child, Some(spawn.id), 100);
        let spans = vec![t1, spawn, c1];
        let mut facts = node(n, NodeStatus::Succeeded);
        facts.cost_usd = 2.0;
        facts.tokens_in = 300;
        let session = build(run(RunStatus::Succeeded), &[facts], &spans, &price);
        let node_out = &session.nodes[0];
        assert!(
            (node_out.rollup.cost_usd - 2.0).abs() < 1e-9,
            "the record wins"
        );
        assert_eq!(node_out.rollup.tokens_in, 300, "what the result recorded");
        let analyst = &node_out.agents[0];
        assert!(
            (analyst.total.cost_usd - 2.0).abs() < 1e-9,
            "subtrees sum to the node"
        );
        assert!((analyst.own.cost_usd - 1.0).abs() < 1e-9);
        assert!((analyst.children[0].total.cost_usd - 1.0).abs() < 1e-9);

        let cached = NodeFacts {
            cached: true,
            tokens_in: 40,
            tokens_out: 8,
            ..node(Uuid::now_v7(), NodeStatus::Succeeded)
        };
        let session = build(run(RunStatus::Succeeded), &[cached], &[], &price);
        assert_eq!(session.nodes[0].rollup.tokens_in, 40);
        assert!(session.nodes[0].agents.is_empty());
        assert_eq!(session.rollup.spans, 0);
        assert_eq!(session.duration_ms, None);
        assert_eq!(
            SessionStatus::from(NodeStatus::Skipped),
            SessionStatus::Skipped
        );
        assert!(!SessionStatus::WaitingApproval.is_terminal());
    }
}
