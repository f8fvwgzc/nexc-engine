//! Structural analysis of a graph (topological order, waves, critical path,
//! cycles, components).

use uuid::Uuid;

use crate::domain::graph::{EdgeKind, GraphAnalysis, GraphEdge, GraphNode};
use crate::dsa::graph::{DiGraph, Indexed};

/// Index of node ids plus the `depends_on` digraph over them.
pub fn dependency_graph(nodes: &[GraphNode], edges: &[GraphEdge]) -> (Indexed<Uuid>, DiGraph) {
    let index = Indexed::new(nodes.iter().map(|n| n.id));
    let graph = index.graph(
        edges
            .iter()
            .filter(|e| e.kind == EdgeKind::DependsOn)
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
    use crate::domain::graph::{EdgeOrigin, Executor, NodeKind, NodeOrigin, NodeStatus};

    pub fn node(title: &str) -> GraphNode {
        GraphNode {
            id: Uuid::now_v7(),
            graph_id: Uuid::nil(),
            title: title.into(),
            content: String::new(),
            kind: NodeKind::Task,
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

    pub fn edge(s: &GraphNode, t: &GraphNode, kind: EdgeKind) -> GraphEdge {
        GraphEdge {
            id: Uuid::now_v7(),
            graph_id: Uuid::nil(),
            source: s.id,
            target: t.id,
            kind,
            origin: EdgeOrigin::User,
            weight: 1.0,
        }
    }

    #[test]
    fn analyzes_dependencies() {
        let (a, b, c, d) = (node("a"), node("b"), node("c"), node("d"));
        let edges = vec![
            edge(&a, &b, EdgeKind::DependsOn),
            edge(&b, &c, EdgeKind::DependsOn),
            edge(&a, &d, EdgeKind::RelatesTo),
        ];
        let r = analyze(&[a.clone(), b.clone(), c.clone(), d.clone()], &edges);
        assert_eq!(r.topo_order.len(), 4);
        assert_eq!(r.critical_path, vec![a.id, b.id, c.id]);
        assert_eq!(r.levels[0], vec![a.id, d.id]);
        assert!(r.cycles.is_empty());
        assert_eq!(r.components, vec![vec![a.id, b.id, c.id, d.id]]);
    }
}
