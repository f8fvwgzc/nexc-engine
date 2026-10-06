//! Plans: LLM-proposed refinements of a graph, and the rules that make a
//! proposal safe to apply.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

use super::graph::{Executor, TITLE_MAX};
use super::ontology::{Ontology, REASON_MAX, slug};
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
    /// Key of a node type: one of the graph's, or one the plan's ontology adds.
    pub kind: String,
    pub agent_role: String,
    pub executor: Executor,
    pub tags: Vec<String>,
}

fn lenient_uuid<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Uuid>, D::Error> {
    let raw: Option<String> = Option::deserialize(d)?;
    Ok(raw.and_then(|s| s.trim().parse().ok()))
}

/// A typed relation between two proposed nodes (by `ref`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct ProposedEdge {
    pub source_ref: String,
    pub target_ref: String,
    /// Key of a relation type; empty means the ontology's dependency relation.
    #[serde(default)]
    #[schema(required = true)]
    pub kind: String,
    /// Why the two nodes are related this way.
    #[serde(default)]
    #[schema(required = true)]
    pub reason: String,
}

/// A plan as returned by the API.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Plan {
    pub id: Uuid,
    pub graph_id: Uuid,
    pub status: PlanStatus,
    pub summary: String,
    /// Node and relation types the plan adds to the graph's ontology.
    pub ontology: Ontology,
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
    /// New node and relation types; types the graph already has are not repeated.
    #[serde(default)]
    pub ontology: Ontology,
    pub nodes: Vec<ProposedNode>,
    pub edges: Vec<ProposedEdge>,
}

/// An existing node as shown to the planner.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextNode {
    pub id: Uuid,
    pub title: String,
    pub kind: String,
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
    pub kind: String,
    #[serde(default)]
    pub reason: String,
}

/// Everything the planner tells the LLM about the graph. Serialised as JSON
/// after [`PLAN_CONTEXT_MARKER`] so that offline providers can read it back.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanContext {
    pub goal: String,
    pub instructions: String,
    /// The graph's current node types and relation types.
    #[serde(default)]
    pub ontology: Ontology,
    pub nodes: Vec<ContextNode>,
    pub edges: Vec<ContextEdge>,
    /// Detected-but-unconfirmed dependencies, as "source title -> target title: reason".
    pub suggestions: Vec<String>,
    pub memories: Vec<String>,
    /// Passages of the workspace's documents that bear on the goal, each
    /// headed by where it comes from ("[file, p. N › section]").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<String>,
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
/// node and edge kinds that resolve to a type of `ontology` or of the plan's
/// own additions (kinds the model used without declaring are declared for
/// it), edges between known refs only, no duplicates and no cycle among
/// blocking edges (edges that would close one are dropped in order). The
/// proposal's ontology is reduced to the types the graph does not have yet.
/// Returns human readable notes about every correction.
pub fn sanitize_proposal(
    mut proposal: PlanProposal,
    existing: &HashSet<Uuid>,
    ontology: &Ontology,
    max_nodes: usize,
) -> (PlanProposal, Vec<String>) {
    let mut notes = Vec::new();
    proposal.ontology.normalize();
    let mut merged = ontology.clone();
    merged.merge(&proposal.ontology);

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
        let kind = slug(&node.kind);
        if merged.node_type(&kind).is_none() {
            if merged.ensure_node_type(&kind) {
                notes.push(format!("declared undeclared node type `{kind}`"));
            } else {
                let fallback = merged.default_node_kind().unwrap_or_default().to_owned();
                notes.push(format!(
                    "node `{}` used unusable type `{}`; using `{fallback}`",
                    node.reference, node.kind
                ));
                node.kind = fallback;
                return !node.kind.is_empty();
            }
        }
        node.kind = kind;
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
    for mut edge in proposal.edges.drain(..) {
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
        let mut kind = slug(&edge.kind);
        if kind.is_empty() {
            kind = merged
                .default_relation()
                .map(|r| r.key.clone())
                .unwrap_or_default();
        }
        if merged.relation_type(&kind).is_none() {
            // An undeclared relation never orders execution: only relations the
            // ontology marks blocking may hold a node back.
            if !merged.ensure_relation_type(&kind, false) {
                notes.push(format!(
                    "dropped edge {} -> {} (unusable relation `{}`)",
                    edge.source_ref, edge.target_ref, edge.kind
                ));
                continue;
            }
            notes.push(format!("declared undeclared relation type `{kind}`"));
        }
        if s == t || !seen_edges.insert((s, t, kind.clone())) {
            continue;
        }
        if merged.relation_type(&kind).is_some_and(|r| r.blocking) {
            if dag.would_create_cycle(s, t) {
                notes.push(format!(
                    "dropped edge {} -> {} (would create a cycle)",
                    edge.source_ref, edge.target_ref
                ));
                continue;
            }
            dag.add_edge(s, t);
        }
        edge.kind = kind;
        edge.reason = edge.reason.trim().chars().take(REASON_MAX).collect();
        kept.push(edge);
    }
    proposal.edges = kept;

    merged
        .node_types
        .retain(|t| ontology.node_type(&t.key).is_none());
    merged
        .relation_types
        .retain(|t| ontology.relation_type(&t.key).is_none());
    proposal.ontology = merged;
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
            kind: "task".into(),
            agent_role: "writer".into(),
            executor: Executor::Llm,
            tags: vec![],
        }
    }

    fn edge(s: &str, t: &str) -> ProposedEdge {
        ProposedEdge {
            source_ref: s.into(),
            target_ref: t.into(),
            kind: "depends_on".into(),
            reason: String::new(),
        }
    }

    #[test]
    fn sanitizes_refs_ids_and_cycles() {
        let known = Uuid::now_v7();
        let unknown = Uuid::now_v7();
        let proposal = PlanProposal {
            summary: "s".into(),
            ontology: Ontology::default(),
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
        let (clean, notes) =
            sanitize_proposal(proposal, &HashSet::from([known]), &Ontology::starter(), 10);
        assert_eq!(clean.nodes.len(), 3);
        assert_eq!(clean.nodes[0].existing_id, Some(known));
        assert_eq!(clean.nodes[1].existing_id, None);
        assert_eq!(
            clean.nodes[2].existing_id, None,
            "existing id may be used once"
        );
        assert_eq!(clean.edges, vec![edge("a", "b"), edge("b", "c")]);
        assert!(notes.iter().any(|n| n.contains("cycle")));
        assert_eq!(
            clean.ontology,
            Ontology::default(),
            "nothing new was needed"
        );
    }

    #[test]
    fn resolves_kinds_against_the_ontology() {
        let mut hypothesis = node("h", None);
        hypothesis.kind = "Hypothesis".into();
        let mut signal = node("s", None);
        signal.kind = "market signal".into();
        let proposal: PlanProposal = serde_json::from_value(serde_json::json!({
            "summary": "s",
            "ontology": {
                "node_types": [{ "key": "hypothesis", "label": "Hypothesis", "stage": 1 }],
                "relation_types": [
                    { "key": "validates", "label": "Validates", "blocking": true },
                    { "key": "depends_on", "label": "redeclared" }
                ]
            },
            "nodes": [hypothesis, signal],
            "edges": [
                { "source_ref": "h", "target_ref": "s", "kind": "validates", "reason": " tested first " },
                { "source_ref": "s", "target_ref": "h", "kind": "validates" },
                { "source_ref": "s", "target_ref": "h", "kind": "contradicts" },
                { "source_ref": "h", "target_ref": "s" }
            ]
        }))
        .unwrap();
        let (clean, notes) = sanitize_proposal(proposal, &HashSet::new(), &Ontology::starter(), 10);
        assert_eq!(clean.nodes[1].kind, "market_signal");
        let kinds: Vec<&str> = clean.edges.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, ["validates", "contradicts", "depends_on"]);
        assert_eq!(clean.edges[0].reason, "tested first");
        let added: Vec<&str> = clean
            .ontology
            .node_types
            .iter()
            .map(|t| t.key.as_str())
            .collect();
        assert_eq!(added, ["hypothesis", "market_signal"]);
        let relations = &clean.ontology.relation_types;
        assert_eq!(relations.len(), 2, "the graph already has depends_on");
        assert!(relations[0].blocking && !relations[1].blocking);
        assert!(notes.iter().any(|n| n.contains("market_signal")));
    }

    #[test]
    fn context_round_trips_through_prompt() {
        let ctx = PlanContext {
            goal: "g".into(),
            instructions: String::new(),
            ontology: Ontology::starter(),
            nodes: vec![],
            edges: vec![],
            suggestions: vec![],
            memories: vec![],
            documents: vec![],
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
