//! Structural analysis of a graph (topological order, waves, critical path,
//! cycles, components).

use uuid::Uuid;

use crate::domain::graph::{GraphAnalysis, GraphEdge, GraphNode};
use crate::dsa::graph::{DiGraph, Indexed};

/// Index of node ids plus the digraph of blocking edges over them.
pub fn dependency_graph(nodes: &[GraphNode], edges: &[GraphEdge]) -> (Indexed<Uuid>, DiGraph) {
    let index = Indexed::new(nodes.iter().map(|n| n.id));
    let graph = index.graph(
        edges
            .iter()
            .filter(|e| e.blocking)
            .map(|e| (e.source, e.target)),
    );
    (index, graph)
}

/// Computes the [`GraphAnalysis`] of a graph.
pub fn analyze(nodes: &[GraphNode], edges: &[GraphEdge]) -> GraphAnalysis {
    let (index, deps) = dependency_graph(nodes, edges);
    let all_edges = index.graph(edges.iter().map(|e| (e.source, e.target)));
    let keys = |groups: Vec<Vec<usize>>| groups.iter().map(|g| index.keys_of(g)).collect();
    GraphAnalysis {
        topo_order: index.keys_of(&deps.topo_sort().unwrap_or_else(|partial| partial)),
        levels: keys(deps.levels()),
        critical_path: index.keys_of(&deps.critical_path()),
        cycles: keys(deps.cycles()),
        components: keys(all_edges.weak_components()),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use chrono::Utc;

    use super::*;
    use crate::domain::graph::{EdgeOrigin, Executor, NodeOrigin, NodeStatus};

    pub fn node(title: &str) -> GraphNode {
        GraphNode {
            id: Uuid::now_v7(),
            graph_id: Uuid::nil(),
            title: title.into(),
            content: String::new(),
            kind: "task".into(),
            tags: vec![],
            x: 0.0,
            y: 0.0,
            status: NodeStatus::Idle,
            agent_role: None,
            executor: Executor::Llm,
            output: None,
            origin: NodeOrigin::User,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// An edge of relation `kind`; only `depends_on` blocks, as in the starter ontology.
    pub fn edge(s: &GraphNode, t: &GraphNode, kind: &str) -> GraphEdge {
        GraphEdge {
            id: Uuid::now_v7(),
            graph_id: Uuid::nil(),
            source: s.id,
            target: t.id,
            kind: kind.into(),
            blocking: kind == "depends_on",
            reason: String::new(),
            origin: EdgeOrigin::User,
            weight: 1.0,
        }
    }

    #[test]
    fn analyzes_dependencies() {
        let (a, b, c, d) = (node("a"), node("b"), node("c"), node("d"));
        let edges = vec![
            edge(&a, &b, "depends_on"),
            edge(&b, &c, "depends_on"),
            edge(&a, &d, "relates_to"),
        ];
        let r = analyze(&[a.clone(), b.clone(), c.clone(), d.clone()], &edges);
        assert_eq!(r.topo_order.len(), 4);
        assert_eq!(r.critical_path, vec![a.id, b.id, c.id]);
        assert_eq!(r.levels[0], vec![a.id, d.id]);
        assert!(r.cycles.is_empty());
        assert_eq!(r.components, vec![vec![a.id, b.id, c.id, d.id]]);
    }
}
