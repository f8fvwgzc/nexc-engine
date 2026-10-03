//! Graph mutations shared by the REST API and the WebSocket. Every change
//! bumps the graph version, is broadcast to the graph's sockets and, for
//! content edits, re-runs dependency detection.

use uuid::Uuid;

use super::analysis::dependency_graph;
use super::deps;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::graph::{
    EdgeKind, EdgeOrigin, Graph, GraphEdge, GraphMeta, GraphNode, MAX_NODES, NodeDraft, NodePatch,
};
use crate::realtime::events::WsMessage;
use crate::repo::{self, OrNotFound};

/// Loads a graph header owned by `owner` (404 otherwise).
pub async fn owned_graph(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
) -> Result<GraphMeta, AppError> {
    repo::graphs::find(&state.db, owner, graph_id)
        .await
        .or_not_found("graph")
}

/// Loads a full graph owned by `owner`.
pub async fn load_graph(state: &AppState, owner: Uuid, graph_id: Uuid) -> Result<Graph, AppError> {
    let meta = owned_graph(state, owner, graph_id).await?;
    let nodes = repo::nodes::list(&state.db, graph_id).await?;
    let edges = repo::edges::list(&state.db, graph_id).await?;
    Ok(meta.into_graph(nodes, edges))
}

/// Bumps the version and tells sockets about the new summary.
pub async fn touched(state: &AppState, graph_id: Uuid) -> Result<(), AppError> {
    repo::graphs::touch(&state.db, graph_id).await?;
    if let Some(graph) = repo::graphs::summary(&state.db, graph_id).await? {
        state
            .hub
            .broadcast(graph_id, WsMessage::GraphUpdated { graph });
    }
    Ok(())
}

/// Adds a node (≤ [`MAX_NODES`] per graph).
pub async fn create_node(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
    draft: NodeDraft,
) -> Result<GraphNode, AppError> {
    owned_graph(state, owner, graph_id).await?;
    if repo::nodes::count(&state.db, graph_id).await? >= MAX_NODES {
        return Err(AppError::Unprocessable(format!(
            "a graph can hold at most {MAX_NODES} nodes"
        )));
    }
    let node = repo::nodes::create(&state.db, Uuid::now_v7(), graph_id, &draft).await?;
    state
        .hub
        .broadcast(graph_id, WsMessage::NodeUpserted { node: node.clone() });
    touched(state, graph_id).await?;
    deps::schedule(state, graph_id);
    Ok(node)
}

/// Applies a partial update to a node.
pub async fn update_node(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
    node_id: Uuid,
    patch: NodePatch,
) -> Result<GraphNode, AppError> {
    owned_graph(state, owner, graph_id).await?;
    let mut node = repo::nodes::find(&state.db, graph_id, node_id)
        .await
        .or_not_found("node")?;
    let text_changed = patch.apply(&mut node);
    let node = repo::nodes::save(&state.db, &node).await?;
    state
        .hub
        .broadcast(graph_id, WsMessage::NodeUpserted { node: node.clone() });
    touched(state, graph_id).await?;
    if text_changed {
        deps::schedule(state, graph_id);
    }
    Ok(node)
}

/// Moves a node (WebSocket drag); ownership was checked when the socket opened.
pub async fn move_node(
    state: &AppState,
    graph_id: Uuid,
    node_id: Uuid,
    x: f64,
    y: f64,
) -> Result<(), AppError> {
    if let Some(node) = repo::nodes::set_position(&state.db, graph_id, node_id, x, y).await? {
        repo::graphs::touch(&state.db, graph_id).await?;
        state
            .hub
            .broadcast(graph_id, WsMessage::NodeUpserted { node });
    }
    Ok(())
}

/// Deletes a node and (by cascade) its edges.
pub async fn delete_node(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
    node_id: Uuid,
) -> Result<(), AppError> {
    owned_graph(state, owner, graph_id).await?;
    if !repo::nodes::delete(&state.db, graph_id, node_id).await? {
        return Err(AppError::NotFound("node"));
    }
    state
        .hub
        .broadcast(graph_id, WsMessage::NodeDeleted { node_id });
    touched(state, graph_id).await?;
    deps::schedule(state, graph_id);
    Ok(())
}

/// Adds an edge. `depends_on` edges that would close a cycle are rejected
/// with 409; the graph row is locked so concurrent inserts cannot race.
pub async fn create_edge(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
    source: Uuid,
    target: Uuid,
    kind: EdgeKind,
) -> Result<GraphEdge, AppError> {
    if source == target {
        return Err(AppError::field(
            "target",
            "an edge cannot connect a node to itself",
        ));
    }
    let mut tx = state.db.begin().await?;
    repo::graphs::lock(&mut *tx, owner, graph_id)
        .await
        .or_not_found("graph")?;
    let nodes = repo::nodes::list(&mut *tx, graph_id).await?;
    if !nodes.iter().any(|n| n.id == source) || !nodes.iter().any(|n| n.id == target) {
        return Err(AppError::NotFound("node"));
    }
    if kind == EdgeKind::DependsOn {
        let edges = repo::edges::list(&mut *tx, graph_id).await?;
        let (index, deps) = dependency_graph(&nodes, &edges);
        let (s, t) = (
            index.get(&source).unwrap_or_default(),
            index.get(&target).unwrap_or_default(),
        );
        if deps.would_create_cycle(s, t) {
            return Err(AppError::Conflict(
                "this dependency would create a cycle".into(),
            ));
        }
    }
    let edge = repo::edges::create(&mut *tx, graph_id, source, target, kind, EdgeOrigin::User)
        .await?
        .ok_or_else(|| AppError::Conflict("this edge already exists".into()))?;
    tx.commit().await?;
    state
        .hub
        .broadcast(graph_id, WsMessage::EdgeUpserted { edge: edge.clone() });
    touched(state, graph_id).await?;
    Ok(edge)
}

/// Deletes an edge.
pub async fn delete_edge(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
    edge_id: Uuid,
) -> Result<(), AppError> {
    owned_graph(state, owner, graph_id).await?;
    if !repo::edges::delete(&state.db, graph_id, edge_id).await? {
        return Err(AppError::NotFound("edge"));
    }
    state
        .hub
        .broadcast(graph_id, WsMessage::EdgeDeleted { edge_id });
    touched(state, graph_id).await?;
    Ok(())
}
