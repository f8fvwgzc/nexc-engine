//! Graphs, nodes and edges.

use chrono::{DateTime, Utc};
use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use super::ontology::Ontology;
use super::string_enum;

/// Maximum node title length in characters.
pub const TITLE_MAX: usize = 200;
/// Maximum node content length in bytes (64 KiB).
pub const CONTENT_MAX_BYTES: usize = 64 * 1024;
/// Maximum number of nodes in one graph.
pub const MAX_NODES: i64 = 500;
/// Maximum graph name length.
pub const GRAPH_NAME_MAX: usize = 200;
/// Maximum graph description / goal length.
pub const GRAPH_TEXT_MAX: usize = 8 * 1024;
/// Maximum number of tags per node and characters per tag.
pub const TAGS_MAX: usize = 20;
/// Maximum characters per tag.
pub const TAG_LEN_MAX: usize = 40;
/// Maximum agent role length.
pub const ROLE_MAX: usize = 64;

string_enum!(
    /// Execution status of a node (last run).
    NodeStatus {
        Idle => "idle",
        Queued => "queued",
        Running => "running",
        Succeeded => "succeeded",
        Failed => "failed",
        Skipped => "skipped",
        Cancelled => "cancelled",
    }
);

string_enum!(
    /// Which executor runs a node.
    Executor {
        Llm => "llm",
        Agent => "agent",
        Symphony => "symphony",
    }
);

string_enum!(
    /// Who created a node.
    NodeOrigin {
        User => "user",
        Plan => "plan",
    }
);

string_enum!(
    /// Who created an edge (`auto` = `[[wikilink]]` dependency detection).
    EdgeOrigin {
        User => "user",
        Auto => "auto",
        Plan => "plan",
    }
);

/// A note / task on the canvas.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GraphNode {
    pub id: Uuid,
    pub graph_id: Uuid,
    pub title: String,
    pub content: String,
    /// Key of a node type of the graph's ontology.
    pub kind: String,
    pub tags: Vec<String>,
    pub x: f64,
    pub y: f64,
    pub status: NodeStatus,
    #[schema(required = true)]
    pub agent_role: Option<String>,
    pub executor: Executor,
    #[schema(required = true)]
    pub output: Option<String>,
    pub origin: NodeOrigin,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A directed edge between two nodes of the same graph.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GraphEdge {
    pub id: Uuid,
    pub graph_id: Uuid,
    pub source: Uuid,
    pub target: Uuid,
    /// Key of a relation type of the graph's ontology.
    pub kind: String,
    /// Copied from the relation type: source must finish before target runs.
    pub blocking: bool,
    /// Why the two nodes are related this way.
    pub reason: String,
    pub origin: EdgeOrigin,
    pub weight: f64,
}

/// Row of the graph list.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct GraphSummary {
    pub id: Uuid,
    #[schema(required = true)]
    pub workspace_id: Option<Uuid>,
    #[schema(required = true)]
    pub team_id: Option<Uuid>,
    pub name: String,
    pub description: String,
    pub node_count: i64,
    pub edge_count: i64,
    pub updated_at: DateTime<Utc>,
}

/// A full graph with its nodes and edges.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct Graph {
    pub id: Uuid,
    /// The workspace the graph belongs to.
    #[schema(required = true)]
    pub workspace_id: Option<Uuid>,
    /// The team it belongs to; `null` for a graph of the whole workspace.
    #[schema(required = true)]
    pub team_id: Option<Uuid>,
    pub name: String,
    pub description: String,
    pub goal: String,
    /// Incremented on every mutation of the graph, its nodes or edges.
    pub version: i64,
    /// The node types and relation types this graph is built from.
    pub ontology: Ontology,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Graph header without nodes and edges.
#[derive(Debug, Clone)]
pub struct GraphMeta {
    pub id: Uuid,
    /// Who created the graph.
    pub owner_id: Uuid,
    pub workspace_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
    pub name: String,
    pub description: String,
    pub goal: String,
    pub version: i64,
    pub ontology: Ontology,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl GraphMeta {
    /// Combines the header with its nodes and edges.
    pub fn into_graph(self, nodes: Vec<GraphNode>, edges: Vec<GraphEdge>) -> Graph {
        Graph {
            id: self.id,
            workspace_id: self.workspace_id,
            team_id: self.team_id,
            name: self.name,
            description: self.description,
            goal: self.goal,
            version: self.version,
            ontology: self.ontology,
            nodes,
            edges,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// Fields of a node to insert.
#[derive(Debug, Clone)]
pub struct NodeDraft {
    pub title: String,
    pub content: String,
    pub kind: String,
    pub tags: Vec<String>,
    pub x: f64,
    pub y: f64,
    pub agent_role: Option<String>,
    pub executor: Executor,
    pub origin: NodeOrigin,
}

/// A partial update of a node; `None` leaves a field unchanged.
#[derive(Debug, Clone, Default)]
pub struct NodePatch {
    pub title: Option<String>,
    pub content: Option<String>,
    pub kind: Option<String>,
    pub tags: Option<Vec<String>>,
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub status: Option<NodeStatus>,
    /// `Some(None)` clears the role.
    pub agent_role: Option<Option<String>>,
    pub executor: Option<Executor>,
    /// `Some(None)` clears the output.
    pub output: Option<Option<String>>,
}

impl NodePatch {
    /// Applies the patch; returns true when title or content changed
    /// (which re-triggers dependency detection).
    pub fn apply(self, node: &mut GraphNode) -> bool {
        let text_changed = self.title.as_ref().is_some_and(|t| *t != node.title)
            || self.content.as_ref().is_some_and(|c| *c != node.content);
        macro_rules! set {
            ($($field:ident),*) => {$(if let Some(v) = self.$field { node.$field = v; })*};
        }
        set!(
            title, content, kind, tags, x, y, status, agent_role, executor, output
        );
        text_changed
    }
}

/// Normalises tags: trimmed, lowercased, deduplicated, non-empty.
pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for t in tags {
        let t = t.trim().to_lowercase();
        if !t.is_empty() && !out.contains(&t) {
            out.push(t);
        }
    }
    out
}

/// A suggested dependency produced by dependency detection.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct EdgeSuggestion {
    pub source: Uuid,
    pub target: Uuid,
    /// Confidence in `[0, 1]`.
    pub score: f64,
    /// Human readable explanation.
    pub reason: String,
}

/// Structural analysis of the `depends_on` graph.
#[derive(Debug, Clone, Default, PartialEq, Serialize, ToSchema)]
pub struct GraphAnalysis {
    /// A topological order of the acyclic part of the graph.
    pub topo_order: Vec<Uuid>,
    /// Parallel execution waves (every node in a wave only depends on earlier waves).
    pub levels: Vec<Vec<Uuid>>,
    /// The longest dependency chain.
    pub critical_path: Vec<Uuid>,
    /// Strongly connected components with more than one node (should be empty).
    pub cycles: Vec<Vec<Uuid>>,
    /// Weakly connected components over all edges.
    pub components: Vec<Vec<Uuid>>,
}

/// Extracts the targets of `[[Title]]` / `[[Title|alias]]` wikilinks,
/// trimmed and lowercased, in order of appearance without duplicates.
pub fn wikilinks(content: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut rest = content;
    while let Some(start) = rest.find("[[") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("]]") else { break };
        let inner = &after[..end];
        let title = inner
            .split('|')
            .next()
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        if !title.is_empty() && !inner.contains('\n') && !found.contains(&title) {
            found.push(title);
        }
        rest = &after[end + 2..];
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wikilinks() {
        let text = "See [[Intro]] and [[ methods | the methods]], again [[intro]]. [[]] [[broken";
        assert_eq!(
            wikilinks(text),
            vec!["intro".to_owned(), "methods".to_owned()]
        );
        assert!(wikilinks("no links").is_empty());
        assert!(wikilinks("[[multi\nline]]").is_empty());
    }

    #[test]
    fn enum_round_trip() {
        for executor in ["llm", "agent", "symphony"] {
            assert_eq!(executor.parse::<Executor>().unwrap().as_str(), executor);
        }
        assert!("nope".parse::<Executor>().is_err());
        assert_eq!(
            serde_json::to_string(&EdgeOrigin::Auto).unwrap(),
            "\"auto\""
        );
    }
}
