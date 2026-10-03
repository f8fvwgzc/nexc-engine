//! The planner: asks the LLM to refine a graph, streams the proposal to the
//! UI node by node, validates it and applies it on request.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde_json::json;
use uuid::Uuid;

use super::analysis::dependency_graph;
use super::json_stream::ArrayScanner;
use super::{credentials, deps, editor};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::graph::{
    EdgeKind, EdgeOrigin, Graph, GraphEdge, GraphNode, MAX_NODES, NodeDraft, NodeOrigin,
    normalize_tags,
};
use crate::domain::plan::{
    ContextEdge, ContextNode, PLAN_CONTEXT_MARKER, PLAN_SCHEMA_NAME, Plan, PlanContext,
    PlanProposal, PlanStatus, ProposedEdge, ProposedNode, sanitize_proposal,
};
use crate::dsa::graph::Indexed;
use crate::llm::{JsonSchema, LlmEvent, LlmRequest, LlmTarget, Message, StopReason};
use crate::memory;
use crate::realtime::events::{SseEvent, WsMessage};
use crate::repo::{self, OrNotFound, plans};

const CONTEXT_CONTENT_CHARS: usize = 2_000;
const EDGE_ANIMATION_DELAY: Duration = Duration::from_millis(40);
const COLUMN_WIDTH: f64 = 320.0;
const ROW_HEIGHT: f64 = 180.0;
const MARGIN: f64 = 80.0;

const SYSTEM_PROMPT: &str = "You are the planning engine of nexc, a tool that executes graphs of notes and \
    tasks with LLM agents. Refine the user's graph into an executable plan: keep every useful existing \
    node (set existing_id to its id), split nodes that are too broad into concrete steps, add missing \
    steps, and connect them with depends_on edges (source must finish before target). Each node must \
    be independently executable by one agent with clear instructions in `content`. Prefer parallel \
    branches where steps are independent. End with exactly one node of kind `output` that produces the \
    final deliverable. Choose each node's executor: `llm` for text-only steps, `agent` for steps \
    that must produce files (documents such as .docx, code, data) or need tools - the final `output` \
    node is `agent` whenever the goal asks for a file - and `symphony` only for coding tasks against a \
    git repository. Use only the given agent roles. `ref` values are short unique identifiers.";

/// JSON schema of [`PlanProposal`] (strict: every object closed, every field required).
pub fn plan_schema() -> serde_json::Value {
    let string = json!({ "type": "string" });
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["summary", "nodes", "edges"],
        "properties": {
            "summary": string,
            "nodes": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["ref", "existing_id", "title", "content", "kind", "agent_role", "executor", "tags"],
                    "properties": {
                        "ref": string,
                        "existing_id": { "anyOf": [{ "type": "string" }, { "type": "null" }] },
                        "title": string,
                        "content": string,
                        "kind": { "type": "string", "enum": ["topic", "task", "research", "code", "document", "output"] },
                        "agent_role": string,
                        "executor": { "type": "string", "enum": ["llm", "agent", "symphony"] },
                        "tags": { "type": "array", "items": string }
                    }
                }
            },
            "edges": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["source_ref", "target_ref"],
                    "properties": { "source_ref": string, "target_ref": string }
                }
            }
        }
    })
}

/// Creates a plan and starts generating it in the background (202 semantics).
pub async fn start(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
    instructions: String,
) -> Result<Plan, AppError> {
    let graph = editor::load_graph(state, owner, graph_id).await?;
    let llm = credentials::require(state, owner).await?;
    let plan = plans::create(&state.db, graph_id, owner, &instructions).await?;
    let (state, plan_id) = (state.clone(), plan.id);
    tokio::spawn(async move {
        state
            .hub
            .publish(graph_id, SseEvent::PlanStarted { plan_id });
        match generate(&state, owner, &graph, plan_id, &instructions, llm.target).await {
            Ok(proposal) => match plans::complete(&state.db, plan_id, &proposal).await {
                Ok(plan) => state.hub.publish(graph_id, SseEvent::PlanReady { plan }),
                Err(err) => {
                    fail(
                        &state,
                        graph_id,
                        plan_id,
                        &format!("cannot store plan: {err}"),
                    )
                    .await
                }
            },
            Err(error) => fail(&state, graph_id, plan_id, &error).await,
        }
    });
    Ok(plan)
}

async fn fail(state: &AppState, graph_id: Uuid, plan_id: Uuid, error: &str) {
    tracing::warn!(%plan_id, error, "plan failed");
    if let Err(err) = plans::fail(&state.db, plan_id, error).await {
        tracing::error!(%plan_id, error = %err, "cannot mark plan failed");
    }
    state.hub.publish(
        graph_id,
        SseEvent::PlanFailed {
            plan_id,
            error: error.to_owned(),
        },
    );
}

async fn build_context(
    state: &AppState,
    owner: Uuid,
    graph: &Graph,
    instructions: &str,
) -> PlanContext {
    let titles: HashMap<Uuid, &str> = graph
        .nodes
        .iter()
        .map(|n| (n.id, n.title.as_str()))
        .collect();
    let suggestions = deps::suggestions(&graph.nodes, &graph.edges)
        .into_iter()
        .take(10)
        .map(|s| {
            format!(
                "{} -> {}: {}",
                titles[&s.source], titles[&s.target], s.reason
            )
        })
        .collect();
    let query = format!("{} {instructions}", graph.goal);
    let memories = memory::retrieve(&state.db, owner, Some(graph.id), &query, 8)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|m| m.content)
        .collect();
    let mut agent_roles: Vec<String> = repo::agents::list(&state.db, owner)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|a| a.role)
        .collect();
    agent_roles.sort();
    agent_roles.dedup();
    PlanContext {
        goal: graph.goal.clone(),
        instructions: instructions.to_owned(),
        nodes: graph
            .nodes
            .iter()
            .map(|n| ContextNode {
                id: n.id,
                title: n.title.clone(),
                kind: n.kind,
                content: n.content.chars().take(CONTEXT_CONTENT_CHARS).collect(),
                tags: n.tags.clone(),
                agent_role: n.agent_role.clone(),
                executor: n.executor,
            })
            .collect(),
        edges: graph
            .edges
            .iter()
            .map(|e| ContextEdge {
                source: e.source,
                target: e.target,
                kind: e.kind,
            })
            .collect(),
        suggestions,
        memories,
        agent_roles,
    }
}

async fn generate(
    state: &AppState,
    owner: Uuid,
    graph: &Graph,
    plan_id: Uuid,
    instructions: &str,
    target: LlmTarget,
) -> Result<PlanProposal, String> {
    let context = build_context(state, owner, graph, instructions).await;
    let prompt = format!(
        "Refine the graph described below.{}\n\n{PLAN_CONTEXT_MARKER}\n{}",
        if instructions.trim().is_empty() {
            String::new()
        } else {
            format!("\nUser instructions: {instructions}")
        },
        serde_json::to_string(&context).map_err(|e| e.to_string())?
    );
    let request = LlmRequest {
        target,
        system: SYSTEM_PROMPT.into(),
        messages: vec![Message::user(prompt)],
        max_tokens: 64_000,
        json_schema: Some(JsonSchema {
            name: PLAN_SCHEMA_NAME,
            schema: plan_schema(),
        }),
        effort: None,
        cacheable: false,
    };
    let text = stream_nodes(state, graph.id, plan_id, request).await?;
    let proposal: PlanProposal = serde_json::from_str(text.trim())
        .map_err(|e| format!("the model returned an invalid plan: {e}"))?;
    let existing: HashSet<Uuid> = graph.nodes.iter().map(|n| n.id).collect();
    let (proposal, notes) = sanitize_proposal(proposal, &existing, MAX_NODES as usize);
    for note in &notes {
        tracing::info!(%plan_id, note, "plan corrected");
    }
    for edge in &proposal.edges {
        state.hub.publish(
            graph.id,
            SseEvent::PlanEdge {
                plan_id,
                edge: edge.clone(),
            },
        );
        tokio::time::sleep(EDGE_ANIMATION_DELAY).await;
    }
    Ok(proposal)
}

/// Streams the completion, publishing each proposed node as soon as its
/// JSON object is complete. Returns the full text.
async fn stream_nodes(
    state: &AppState,
    graph_id: Uuid,
    plan_id: Uuid,
    request: LlmRequest,
) -> Result<String, String> {
    use futures::StreamExt;
    let mut stream = state.llm.stream(request);
    let mut scanner = ArrayScanner::new("nodes");
    let mut text = String::new();
    while let Some(event) = stream.next().await {
        match event.map_err(|e| e.to_string())? {
            LlmEvent::Text(delta) => {
                text.push_str(&delta);
                for raw in scanner.scan(&text) {
                    if let Ok(node) = serde_json::from_str::<ProposedNode>(raw) {
                        state
                            .hub
                            .publish(graph_id, SseEvent::PlanNode { plan_id, node });
                    }
                }
            }
            LlmEvent::Usage(_) => {}
            LlmEvent::Done(StopReason::MaxTokens) => {
                return Err("the plan was cut off (max_tokens)".into());
            }
            LlmEvent::Done(StopReason::EndTurn) => return Ok(text),
        }
    }
    Err("the LLM stream ended unexpectedly".into())
}

/// Applies a ready plan in one transaction: upserts its nodes, replaces the
/// graph's plan-origin edges and lays out new nodes by topological level.
pub async fn apply(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
    plan_id: Uuid,
) -> Result<Graph, AppError> {
    let mut tx = state.db.begin().await?;
    repo::graphs::lock(&mut *tx, owner, graph_id)
        .await
        .or_not_found("graph")?;
    let plan = plans::find(&mut *tx, graph_id, plan_id)
        .await
        .or_not_found("plan")?;
    if !plans::transition(&mut *tx, plan_id, PlanStatus::Ready, PlanStatus::Applied).await? {
        return Err(AppError::Conflict(format!(
            "plan is {}, only ready plans can be applied",
            plan.status
        )));
    }
    let existing = repo::nodes::list(&mut *tx, graph_id).await?;
    let ids = assign_ids(&plan.nodes, &existing);
    let new_count = plan
        .nodes
        .iter()
        .filter(|n| !existing.iter().any(|e| Some(e.id) == n.existing_id))
        .count();
    if existing.len() + new_count > MAX_NODES as usize {
        return Err(AppError::Unprocessable(format!(
            "applying this plan would exceed {MAX_NODES} nodes"
        )));
    }

    let removed = repo::edges::delete_by_origin(&mut *tx, graph_id, EdgeOrigin::Plan).await?;
    let mut edges = repo::edges::list(&mut *tx, graph_id).await?;
    let layout = layout_new_nodes(&plan, &ids, &existing, &edges);
    let mut upserted = Vec::new();
    for proposed in &plan.nodes {
        let id = ids[&proposed.reference];
        let node = match existing.iter().find(|n| n.id == id) {
            Some(current) => repo::nodes::save(&mut *tx, &refined(current, proposed)).await?,
            None => {
                let (x, y) = layout.get(&id).copied().unwrap_or((MARGIN, MARGIN));
                repo::nodes::create(&mut *tx, id, graph_id, &draft(proposed, x, y)).await?
            }
        };
        upserted.push(node);
    }
    let untouched = existing
        .iter()
        .filter(|n| !upserted.iter().any(|u| u.id == n.id))
        .cloned();
    let all_nodes: Vec<GraphNode> = untouched.chain(upserted.iter().cloned()).collect();
    let mut added = Vec::new();
    for ProposedEdge {
        source_ref,
        target_ref,
    } in &plan.edges
    {
        let (Some(&source), Some(&target)) = (ids.get(source_ref), ids.get(target_ref)) else {
            continue;
        };
        let (index, deps) = dependency_graph(&all_nodes, &edges);
        let (Some(s), Some(t)) = (index.get(&source), index.get(&target)) else {
            continue;
        };
        if deps.would_create_cycle(s, t) {
            continue;
        }
        if let Some(edge) = repo::edges::create(
            &mut *tx,
            graph_id,
            source,
            target,
            EdgeKind::DependsOn,
            EdgeOrigin::Plan,
        )
        .await?
        {
            edges.push(edge.clone());
            added.push(edge);
        }
    }
    repo::graphs::touch(&mut *tx, graph_id).await?;
    tx.commit().await?;

    for edge_id in removed {
        state
            .hub
            .broadcast(graph_id, WsMessage::EdgeDeleted { edge_id });
    }
    for node in upserted {
        state
            .hub
            .broadcast(graph_id, WsMessage::NodeUpserted { node });
    }
    for edge in added {
        state
            .hub
            .broadcast(graph_id, WsMessage::EdgeUpserted { edge });
    }
    editor::touched(state, graph_id).await?;
    deps::schedule(state, graph_id);
    editor::load_graph(state, owner, graph_id).await
}

/// Maps every plan ref to the id of the existing node it refines or a new id.
fn assign_ids(nodes: &[ProposedNode], existing: &[GraphNode]) -> HashMap<String, Uuid> {
    nodes
        .iter()
        .map(|n| {
            let id = n
                .existing_id
                .filter(|id| existing.iter().any(|e| e.id == *id))
                .unwrap_or_else(Uuid::now_v7);
            (n.reference.clone(), id)
        })
        .collect()
}

fn refined(current: &GraphNode, p: &ProposedNode) -> GraphNode {
    GraphNode {
        title: p.title.clone(),
        content: p.content.clone(),
        kind: p.kind,
        tags: normalize_tags(&p.tags),
        agent_role: Some(p.agent_role.clone()).filter(|r| !r.trim().is_empty()),
        executor: p.executor,
        ..current.clone()
    }
}

fn draft(p: &ProposedNode, x: f64, y: f64) -> NodeDraft {
    NodeDraft {
        title: p.title.clone(),
        content: p.content.clone(),
        kind: p.kind,
        tags: normalize_tags(&p.tags),
        x,
        y,
        agent_role: Some(p.agent_role.clone()).filter(|r| !r.trim().is_empty()),
        executor: p.executor,
        origin: NodeOrigin::Plan,
    }
}

/// Positions for the plan's new nodes: one column per topological level of
/// the resulting graph, in a band below the existing nodes.
fn layout_new_nodes(
    plan: &Plan,
    ids: &HashMap<String, Uuid>,
    existing: &[GraphNode],
    edges: &[GraphEdge],
) -> HashMap<Uuid, (f64, f64)> {
    let index = Indexed::new(
        existing
            .iter()
            .map(|n| n.id)
            .chain(plan.nodes.iter().map(|n| ids[&n.reference])),
    );
    let graph = index.graph(
        edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DependsOn)
            .map(|e| (e.source, e.target))
            .chain(
                plan.edges
                    .iter()
                    .filter_map(|e| Some((*ids.get(&e.source_ref)?, *ids.get(&e.target_ref)?))),
            ),
    );
    let existing_ids: HashSet<Uuid> = existing.iter().map(|n| n.id).collect();
    let base_y = existing
        .iter()
        .map(|n| n.y)
        .fold(None, |m: Option<f64>, y| Some(m.map_or(y, |m| m.max(y))));
    let base_y = base_y.map_or(MARGIN, |y| y + ROW_HEIGHT + 40.0);
    let mut positions = HashMap::new();
    for (level, wave) in graph.levels().iter().enumerate() {
        let new_ids = wave
            .iter()
            .map(|&v| index.key(v))
            .filter(|id| !existing_ids.contains(id));
        for (row, id) in new_ids.enumerate() {
            positions.insert(
                id,
                (
                    MARGIN + COLUMN_WIDTH * level as f64,
                    base_y + ROW_HEIGHT * row as f64,
                ),
            );
        }
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::graph::Executor;
    use crate::domain::graph::NodeKind;

    fn proposed(r: &str) -> ProposedNode {
        ProposedNode {
            reference: r.into(),
            existing_id: None,
            title: r.into(),
            content: String::new(),
            kind: NodeKind::Task,
            agent_role: "writer".into(),
            executor: Executor::Llm,
            tags: vec![],
        }
    }

    #[test]
    fn schema_is_strict() {
        let schema = plan_schema();
        assert_eq!(schema["additionalProperties"], false);
        let item = &schema["properties"]["nodes"]["items"];
        assert_eq!(item["additionalProperties"], false);
        assert_eq!(
            item["required"].as_array().unwrap().len(),
            item["properties"].as_object().unwrap().len()
        );
    }

    #[test]
    fn lays_out_new_nodes_by_level() {
        let plan = Plan {
            id: Uuid::nil(),
            graph_id: Uuid::nil(),
            status: PlanStatus::Ready,
            summary: String::new(),
            nodes: vec![proposed("a"), proposed("b"), proposed("c")],
            edges: vec![
                ProposedEdge {
                    source_ref: "a".into(),
                    target_ref: "b".into(),
                },
                ProposedEdge {
                    source_ref: "a".into(),
                    target_ref: "c".into(),
                },
            ],
            error: None,
            created_at: chrono::Utc::now(),
        };
        let ids = assign_ids(&plan.nodes, &[]);
        let pos = layout_new_nodes(&plan, &ids, &[], &[]);
        assert_eq!(pos[&ids["a"]], (MARGIN, MARGIN));
        assert_eq!(pos[&ids["b"]], (MARGIN + COLUMN_WIDTH, MARGIN));
        assert_eq!(pos[&ids["c"]], (MARGIN + COLUMN_WIDTH, MARGIN + ROW_HEIGHT));
    }
}
