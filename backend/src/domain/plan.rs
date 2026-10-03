//! Plans: LLM-proposed refinements of a graph, and the rules that make a
//! proposal safe to apply.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::graph::{EdgeKind, Executor, NodeKind, TITLE_MAX};
use super::string_enum;
use crate::dsa::graph::DiGraph;

/// Name of the structured-output schema used by the planner.
pub const PLAN_SCHEMA_NAME: &str = "plan_proposal";

/// Marker that precedes the JSON [`PlanContext`] in the planner's user message.
pub const PLAN_CONTEXT_MARKER: &str = "PLAN_CONTEXT_JSON:";

string_enum!(
    /// Lifecycle of a plan.
    PlanStatus {
        Streaming => "streaming",
        Ready => "ready",
        Failed => "failed",
        Applied => "applied",
    }
);

/// A node in a plan proposal. `ref` is a plan-local identifier used by edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ProposedNode {
    #[serde(rename = "ref")]
    pub reference: String,
    /// Id of the existing node this refines, or `null` for a new node.
    /// Malformed ids from the model are read as `null`.
    #[serde(default, deserialize_with = "lenient_uuid")]
    #[schema(required = true)]
    pub existing_id: Option<Uuid>,
    pub title: String,
    pub content: String,
    pub kind: NodeKind,
    pub agent_role: String,
    pub executor: Executor,
    pub tags: Vec<String>,
}

fn lenient_uuid<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Uuid>, D::Error> {
    let raw: Option<String> = Option::deserialize(d)?;
    Ok(raw.and_then(|s| s.trim().parse().ok()))
}

/// A `depends_on` edge between two proposed nodes (by `ref`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct ProposedEdge {
    pub source_ref: String,
    pub target_ref: String,
}

/// A plan as returned by the API.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Plan {
    pub id: Uuid,
    pub graph_id: Uuid,
    pub status: PlanStatus,
    pub summary: String,
    pub nodes: Vec<ProposedNode>,
    pub edges: Vec<ProposedEdge>,
    #[schema(required = true)]
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// The structured output the planner asks the LLM for.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PlanProposal {
    pub summary: String,
    pub nodes: Vec<ProposedNode>,
    pub edges: Vec<ProposedEdge>,
}

/// An existing node as shown to the planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextNode {
    pub id: Uuid,
    pub title: String,
    pub kind: NodeKind,
    pub content: String,
    pub tags: Vec<String>,
    pub agent_role: Option<String>,
    pub executor: Executor,
}

/// An existing edge as shown to the planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEdge {
    pub source: Uuid,
    pub target: Uuid,
    pub kind: EdgeKind,
}

/// Everything the planner tells the LLM about the graph. Serialised as JSON
/// after [`PLAN_CONTEXT_MARKER`] so that offline providers can read it back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanContext {
    pub goal: String,
    pub instructions: String,
    pub nodes: Vec<ContextNode>,
    pub edges: Vec<ContextEdge>,
    /// Detected-but-unconfirmed dependencies, as "source title -> target title: reason".
    pub suggestions: Vec<String>,
    pub memories: Vec<String>,
    /// Agent roles available in the user's organisation.
    pub agent_roles: Vec<String>,
}

impl PlanContext {
    /// Finds and parses the context embedded in a planner prompt.
    pub fn extract(prompt: &str) -> Option<PlanContext> {
        let start = prompt.find(PLAN_CONTEXT_MARKER)? + PLAN_CONTEXT_MARKER.len();
        serde_json::from_str(prompt[start..].trim()).ok()
    }
}

/// Makes an LLM proposal safe to apply: unique non-empty refs, bounded
/// titles, `existing_id`s that really exist in the graph (each used once),
/// edges between known refs only, no duplicates and no cycles (edges that
/// would close a cycle are dropped in order). Returns human readable notes
/// about every correction.
pub fn sanitize_proposal(
    mut proposal: PlanProposal,
    existing: &HashSet<Uuid>,
    max_nodes: usize,
) -> (PlanProposal, Vec<String>) {
    let mut notes = Vec::new();
    let mut seen_refs = HashSet::new();
    let mut used_existing = HashSet::new();
    proposal.nodes.retain_mut(|node| {
        node.reference = node.reference.trim().to_owned();
        node.title = node.title.trim().chars().take(TITLE_MAX).collect();
        if node.reference.is_empty()
            || node.title.is_empty()
            || !seen_refs.insert(node.reference.clone())
        {
            notes.push(format!(
                "dropped node with empty or duplicate ref `{}`",
                node.reference
            ));
            return false;
        }
        if let Some(id) = node.existing_id
            && (!existing.contains(&id) || !used_existing.insert(id))
        {
            notes.push(format!(
                "node `{}` referenced unknown node {id}; treated as new",
                node.reference
            ));
            node.existing_id = None;
        }
        true
    });
    if proposal.nodes.len() > max_nodes {
        notes.push(format!("truncated proposal to {max_nodes} nodes"));
        proposal.nodes.truncate(max_nodes);
    }

    let index: HashMap<&str, usize> = proposal
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.reference.as_str(), i))
        .collect();
    let mut dag = DiGraph::new(proposal.nodes.len());
    let mut kept = Vec::new();
    let mut seen_edges = HashSet::new();
    for edge in proposal.edges.drain(..) {
        let (Some(&s), Some(&t)) = (
            index.get(edge.source_ref.as_str()),
            index.get(edge.target_ref.as_str()),
        ) else {
            notes.push(format!(
                "dropped edge {} -> {} (unknown ref)",
                edge.source_ref, edge.target_ref
            ));
            continue;
        };
        if s == t || !seen_edges.insert((s, t)) {
            continue;
        }
        if dag.would_create_cycle(s, t) {
            notes.push(format!(
                "dropped edge {} -> {} (would create a cycle)",
                edge.source_ref, edge.target_ref
            ));
            continue;
        }
        dag.add_edge(s, t);
        kept.push(edge);
    }
    proposal.edges = kept;
    (proposal, notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(r: &str, existing: Option<Uuid>) -> ProposedNode {
        ProposedNode {
            reference: r.into(),
            existing_id: existing,
            title: format!("Node {r}"),
            content: String::new(),
            kind: NodeKind::Task,
            agent_role: "writer".into(),
            executor: Executor::Llm,
            tags: vec![],
        }
    }

    fn edge(s: &str, t: &str) -> ProposedEdge {
        ProposedEdge {
            source_ref: s.into(),
            target_ref: t.into(),
        }
    }

    #[test]
    fn sanitizes_refs_ids_and_cycles() {
        let known = Uuid::now_v7();
        let unknown = Uuid::now_v7();
        let proposal = PlanProposal {
            summary: "s".into(),
            nodes: vec![
                node("a", Some(known)),
                node("b", Some(unknown)),
                node("a", None),
                node("c", Some(known)),
            ],
            edges: vec![
                edge("a", "b"),
                edge("b", "c"),
                edge("c", "a"),
                edge("a", "zzz"),
                edge("a", "b"),
            ],
        };
        let (clean, notes) = sanitize_proposal(proposal, &HashSet::from([known]), 10);
        assert_eq!(clean.nodes.len(), 3);
        assert_eq!(clean.nodes[0].existing_id, Some(known));
        assert_eq!(clean.nodes[1].existing_id, None);
        assert_eq!(
            clean.nodes[2].existing_id, None,
            "existing id may be used once"
        );
        assert_eq!(clean.edges, vec![edge("a", "b"), edge("b", "c")]);
        assert!(notes.iter().any(|n| n.contains("cycle")));
    }

    #[test]
    fn context_round_trips_through_prompt() {
        let ctx = PlanContext {
            goal: "g".into(),
            instructions: String::new(),
            nodes: vec![],
            edges: vec![],
            suggestions: vec![],
            memories: vec![],
            agent_roles: vec!["writer".into()],
        };
        let prompt = format!(
            "Refine this.\n{PLAN_CONTEXT_MARKER}\n{}",
            serde_json::to_string(&ctx).unwrap()
        );
        assert_eq!(PlanContext::extract(&prompt).unwrap().goal, "g");
        assert!(PlanContext::extract("nothing").is_none());
    }

    #[test]
    fn ref_is_serialized_as_ref() {
        let json = serde_json::to_value(node("x", None)).unwrap();
        assert_eq!(json["ref"], "x");
        let mut v = json.clone();
        v["existing_id"] = serde_json::json!("not-a-uuid");
        assert_eq!(
            serde_json::from_value::<ProposedNode>(v)
                .unwrap()
                .existing_id,
            None
        );
    }
}
