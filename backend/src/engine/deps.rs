//! Dependency detection.
//!
//! 1. **Wikilinks** – `[[Title]]` in a node's content creates an `auto`
//!    `depends_on` edge from the node titled `Title` to the linking node.
//!    Auto edges whose link disappeared are removed.
//! 2. **Suggestions** – pairs of nodes with similar content (C kernel
//!    embedding cosine blended with BM25) are suggested as dependencies;
//!    near-duplicates (MinHash Jaccard ≥ 0.8) are skipped. The more general
//!    or earlier node becomes the source.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use uuid::Uuid;

use super::analysis::dependency_graph;
use crate::app::AppState;
use crate::domain::graph::{
    EdgeKind, EdgeOrigin, EdgeSuggestion, GraphEdge, GraphNode, NodeKind, wikilinks,
};
use crate::dsa::bm25::Bm25Index;
use crate::kernel;
use crate::realtime::events::WsMessage;
use crate::repo;

const MIN_SCORE: f64 = 0.3;
const DUPLICATE_JACCARD: f64 = 0.8;
const MAX_SUGGESTIONS: usize = 20;
const DEBOUNCE: Duration = Duration::from_millis(150);

/// `(source, target)` pairs implied by wikilinks.
pub fn wikilink_pairs(nodes: &[GraphNode]) -> Vec<(Uuid, Uuid)> {
    let by_title: HashMap<String, Uuid> = nodes
        .iter()
        .map(|n| (n.title.trim().to_lowercase(), n.id))
        .collect();
    nodes
        .iter()
        .flat_map(|target| {
            wikilinks(&target.content)
                .into_iter()
                .filter_map(|link| by_title.get(&link).copied())
                .filter(move |&source| source != target.id)
                .map(move |source| (source, target.id))
        })
        .collect()
}

fn kind_rank(kind: NodeKind) -> u8 {
    match kind {
        NodeKind::Topic => 0,
        NodeKind::Research => 1,
        NodeKind::Task | NodeKind::Code => 2,
        NodeKind::Document => 3,
        NodeKind::Output => 4,
    }
}

fn mentions(haystack: &GraphNode, needle: &GraphNode) -> bool {
    needle.title.chars().count() >= 4
        && haystack
            .content
            .to_lowercase()
            .contains(&needle.title.to_lowercase())
}

/// True when `a` should be the source of an edge between `a` and `b`: the
/// node mentioned by the other, then the more general kind, then the older.
fn is_source(a: &GraphNode, b: &GraphNode) -> bool {
    match (mentions(b, a), mentions(a, b)) {
        (true, false) => return true,
        (false, true) => return false,
        _ => {}
    }
    (kind_rank(a.kind), a.created_at) <= (kind_rank(b.kind), b.created_at)
}

/// Ranked dependency suggestions for unconnected node pairs.
pub fn suggestions(nodes: &[GraphNode], edges: &[GraphEdge]) -> Vec<EdgeSuggestion> {
    let docs: Vec<String> = nodes
        .iter()
        .map(|n| format!("{}\n{}", n.title, n.content))
        .collect();
    let embeddings: Vec<_> = docs.iter().map(|d| kernel::embed(d)).collect();
    let signatures: Vec<_> = docs.iter().map(|d| kernel::minhash(d)).collect();
    let bm25 = Bm25Index::build(&docs);
    let lexical: Vec<Vec<f64>> = docs.iter().map(|d| bm25.normalized_scores(d)).collect();
    let connected: HashSet<(Uuid, Uuid)> = edges
        .iter()
        .flat_map(|e| [(e.source, e.target), (e.target, e.source)])
        .collect();
    let (_, deps) = dependency_graph(nodes, edges);

    let mut out = Vec::new();
    for i in 0..nodes.len() {
        for j in i + 1..nodes.len() {
            if connected.contains(&(nodes[i].id, nodes[j].id))
                || kernel::jaccard(&signatures[i], &signatures[j]) >= DUPLICATE_JACCARD
            {
                continue;
            }
            let cosine = f64::from(kernel::dot(&embeddings[i], &embeddings[j])).max(0.0);
            let lex = lexical[i][j].max(lexical[j][i]);
            let score = 0.6 * cosine + 0.4 * lex;
            if score < MIN_SCORE {
                continue;
            }
            // Vertex indices of `deps` equal node positions (ids are unique).
            let (s, t) = if is_source(&nodes[i], &nodes[j]) {
                (i, j)
            } else {
                (j, i)
            };
            if deps.would_create_cycle(s, t) {
                continue;
            }
            let shared = bm25.shared_terms(&docs[s], t, 3);
            let reason = if shared.is_empty() {
                format!("similar content ({:.0}% match)", score * 100.0)
            } else {
                format!(
                    "similar content ({:.0}% match); shared terms: {}",
                    score * 100.0,
                    shared.join(", ")
                )
            };
            out.push(EdgeSuggestion {
                source: nodes[s].id,
                target: nodes[t].id,
                score: (score * 1000.0).round() / 1000.0,
                reason,
            });
        }
    }
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    out.truncate(MAX_SUGGESTIONS);
    out
}

/// Schedules dependency detection for a graph, coalescing bursts of edits.
pub fn schedule(state: &AppState, graph_id: Uuid) {
    if !state.engine.mark_deps_pending(graph_id) {
        return;
    }
    let state = state.clone();
    tokio::spawn(async move {
        tokio::time::sleep(DEBOUNCE).await;
        state.engine.clear_deps_pending(graph_id);
        if let Err(err) = detect(&state, graph_id).await {
            tracing::warn!(%graph_id, error = %err, "dependency detection failed");
        }
    });
}

/// Syncs wikilink edges and pushes suggestions to the graph's sockets.
pub async fn detect(state: &AppState, graph_id: Uuid) -> anyhow::Result<()> {
    let nodes = repo::nodes::list(&state.db, graph_id).await?;
    let mut edges = repo::edges::list(&state.db, graph_id).await?;
    let changed = sync_wikilinks(state, graph_id, &nodes, &mut edges).await?;
    if changed {
        repo::graphs::touch(&state.db, graph_id).await?;
        if let Some(summary) = repo::graphs::summary(&state.db, graph_id).await? {
            state
                .hub
                .broadcast(graph_id, WsMessage::GraphUpdated { graph: summary });
        }
    }
    state.hub.broadcast(
        graph_id,
        WsMessage::Suggestions {
            items: suggestions(&nodes, &edges),
        },
    );
    Ok(())
}

async fn sync_wikilinks(
    state: &AppState,
    graph_id: Uuid,
    nodes: &[GraphNode],
    edges: &mut Vec<GraphEdge>,
) -> anyhow::Result<bool> {
    let wanted: HashSet<(Uuid, Uuid)> = wikilink_pairs(nodes).into_iter().collect();
    let mut changed = false;
    let stale: Vec<Uuid> = edges
        .iter()
        .filter(|e| e.origin == EdgeOrigin::Auto && !wanted.contains(&(e.source, e.target)))
        .map(|e| e.id)
        .collect();
    for id in stale {
        if repo::edges::delete(&state.db, graph_id, id).await? {
            state
                .hub
                .broadcast(graph_id, WsMessage::EdgeDeleted { edge_id: id });
            changed = true;
        }
    }
    edges.retain(|e| e.origin != EdgeOrigin::Auto || wanted.contains(&(e.source, e.target)));

    let existing: HashSet<(Uuid, Uuid)> = edges
        .iter()
        .filter(|e| e.kind == EdgeKind::DependsOn)
        .map(|e| (e.source, e.target))
        .collect();
    for (source, target) in wanted.into_iter().filter(|p| !existing.contains(p)) {
        let (index, deps) = dependency_graph(nodes, edges);
        let (Some(s), Some(t)) = (index.get(&source), index.get(&target)) else {
            continue;
        };
        if deps.would_create_cycle(s, t) {
            continue;
        }
        if let Some(edge) = repo::edges::create(
            &state.db,
            graph_id,
            source,
            target,
            EdgeKind::DependsOn,
            EdgeOrigin::Auto,
        )
        .await?
        {
            state
                .hub
                .broadcast(graph_id, WsMessage::EdgeUpserted { edge: edge.clone() });
            edges.push(edge);
            changed = true;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::analysis::tests::{edge, node};

    #[test]
    fn wikilinks_point_from_referenced_to_referencing() {
        let a = node("Intro");
        let mut b = node("Body");
        b.content = "Builds on [[intro]] and [[Missing]] and [[Body]].".into();
        assert_eq!(wikilink_pairs(&[a.clone(), b.clone()]), vec![(a.id, b.id)]);
    }

    #[test]
    fn suggests_similar_unconnected_nodes() {
        let mut a = node("Research Rust async runtimes");
        a.kind = NodeKind::Research;
        a.content = "Compare tokio and async-std schedulers, work stealing and io drivers.".into();
        let mut b = node("Write runtime comparison");
        b.kind = NodeKind::Document;
        b.content =
            "Write a report comparing tokio and async-std schedulers and their io drivers.".into();
        let mut c = node("Bake bread");
        c.content = "Sourdough starter, flour, water and patience.".into();
        let s = suggestions(&[b.clone(), a.clone(), c.clone()], &[]);
        assert_eq!(s.len(), 1, "{s:?}");
        assert_eq!(
            (s[0].source, s[0].target),
            (a.id, b.id),
            "research feeds the document"
        );
        assert!(s[0].reason.contains("shared terms"), "{}", s[0].reason);
        assert!(
            suggestions(
                &[a.clone(), b.clone()],
                &[edge(&a, &b, EdgeKind::RelatesTo)]
            )
            .is_empty()
        );
    }

    #[test]
    fn skips_near_duplicates() {
        let mut a = node("Draft");
        a.content = "the quick brown fox jumps over the lazy dog again and again".into();
        let mut b = node("Draft");
        b.content = a.content.clone();
        assert!(suggestions(&[a, b], &[]).is_empty());
    }
}
