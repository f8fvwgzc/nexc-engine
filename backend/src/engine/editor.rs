//! Graph mutations shared by the REST API and the WebSocket. Every change
//! bumps the graph version, is broadcast to the graph's sockets and, for
//! content edits, re-runs dependency detection.

use uuid::Uuid;

use super::analysis::dependency_graph;
use super::deps;
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::graph::{
    EdgeOrigin, Executor, Graph, GraphEdge, GraphMeta, GraphNode, MAX_NODES, NodeDraft, NodeOrigin,
    NodePatch,
};
use crate::domain::ontology::Ontology;
use crate::domain::validation::{FieldErrors, Validate};
use crate::realtime::events::WsMessage;
use crate::repo::{self, OrNotFound};

/// A node as a user asks for it; what is left out comes from the ontology.
#[derive(Debug, Clone)]
pub struct NewNode {
    pub title: String,
    pub content: String,
    pub kind: Option<String>,
    pub tags: Vec<String>,
    pub x: f64,
    pub y: f64,
    pub agent_role: Option<String>,
    pub executor: Option<Executor>,
}

/// Where a new graph goes: the workspace and, optionally, the team.
#[derive(Debug, Clone, Copy)]
pub struct GraphHome {
    pub workspace_id: Uuid,
    pub team_id: Option<Uuid>,
}

/// Decides where `user` may create a graph. A team graph needs team
/// membership; a workspace graph needs to be more than a guest. With neither
/// named, the graph goes to the user's default workspace.
pub async fn graph_home(
    state: &AppState,
    user: Uuid,
    workspace_id: Option<Uuid>,
    team_id: Option<Uuid>,
) -> Result<GraphHome, AppError> {
    if let Some(team_id) = team_id {
        let (team_workspace, role) = repo::teams::membership(&state.db, user, team_id)
            .await
            .or_not_found("team")?;
        if role.is_none() || workspace_id.is_some_and(|w| w != team_workspace) {
            // A team the caller is not in is indistinguishable from a missing one.
            return Err(AppError::NotFound("team"));
        }
        return Ok(GraphHome {
            workspace_id: team_workspace,
            team_id: Some(team_id),
        });
    }
    let workspace_id = match workspace_id {
        Some(id) => {
            let role = repo::workspaces::role_of(&state.db, id, user)
                .await
                .or_not_found("workspace")?;
            if !role.is_member() {
                return Err(AppError::Forbidden(
                    "guests can only create graphs in the teams they belong to".into(),
                ));
            }
            id
        }
        None => repo::workspaces::default_for(&state.db, user)
            .await?
            .ok_or_else(|| {
                AppError::Unprocessable(
                    "you are a guest everywhere; name the team to create the graph in".into(),
                )
            })?,
    };
    Ok(GraphHome {
        workspace_id,
        team_id: None,
    })
}

/// Loads a graph header `owner` may work on (404 otherwise).
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

/// 422 unless `kind` is a node type of `ontology`.
fn check_node_kind(ontology: &Ontology, kind: &str) -> Result<(), AppError> {
    if ontology.node_type(kind).is_some() {
        return Ok(());
    }
    Err(AppError::field(
        "kind",
        format!("`{kind}` is not a node type of this graph's ontology"),
    ))
}

/// Adds a node (≤ [`MAX_NODES`] per graph). `kind`, `executor` and the
/// role default to what the graph's ontology says.
pub async fn create_node(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
    new: NewNode,
) -> Result<GraphNode, AppError> {
    let ontology = owned_graph(state, owner, graph_id).await?.ontology;
    let kind = match new.kind {
        Some(kind) => kind,
        None => ontology
            .default_node_kind()
            .ok_or_else(|| AppError::field("kind", "this graph's ontology has no node types"))?
            .to_owned(),
    };
    check_node_kind(&ontology, &kind)?;
    let executor = new
        .executor
        .or(ontology.node_type(&kind).map(|t| t.default_executor))
        .unwrap_or(Executor::Llm);
    let draft = NodeDraft {
        title: new.title,
        content: new.content,
        kind,
        tags: new.tags,
        x: new.x,
        y: new.y,
        agent_role: new.agent_role,
        executor,
        origin: NodeOrigin::User,
    };
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
    let ontology = owned_graph(state, owner, graph_id).await?.ontology;
    if let Some(kind) = &patch.kind {
        check_node_kind(&ontology, kind)?;
    }
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

/// Adds an edge of relation `kind` (the ontology's default when `None`).
/// Edges of a blocking relation that would close a cycle are rejected with
/// 409; the graph row is locked so concurrent inserts cannot race.
pub async fn create_edge(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
    source: Uuid,
    target: Uuid,
    kind: Option<String>,
    reason: &str,
) -> Result<GraphEdge, AppError> {
    if source == target {
        return Err(AppError::field(
            "target",
            "an edge cannot connect a node to itself",
        ));
    }
    let mut tx = state.db.begin().await?;
    let ontology = repo::graphs::lock(&mut *tx, owner, graph_id)
        .await
        .or_not_found("graph")?
        .ontology;
    let relation = match &kind {
        Some(kind) => ontology.relation_type(kind),
        None => ontology.default_relation(),
    }
    .ok_or_else(|| {
        AppError::field(
            "kind",
            format!(
                "`{}` is not a relation type of this graph's ontology",
                kind.as_deref().unwrap_or_default()
            ),
        )
    })?;
    let nodes = repo::nodes::list(&mut *tx, graph_id).await?;
    if !nodes.iter().any(|n| n.id == source) || !nodes.iter().any(|n| n.id == target) {
        return Err(AppError::NotFound("node"));
    }
    if relation.blocking {
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
    let edge = repo::edges::create(
        &mut *tx,
        graph_id,
        source,
        target,
        relation,
        reason,
        EdgeOrigin::User,
    )
    .await?
    .ok_or_else(|| AppError::Conflict("this edge already exists".into()))?;
    tx.commit().await?;
    state
        .hub
        .broadcast(graph_id, WsMessage::EdgeUpserted { edge: edge.clone() });
    touched(state, graph_id).await?;
    Ok(edge)
}

/// Rewrites why an edge exists.
pub async fn update_edge_reason(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
    edge_id: Uuid,
    reason: &str,
) -> Result<GraphEdge, AppError> {
    owned_graph(state, owner, graph_id).await?;
    let edge = repo::edges::set_reason(&state.db, graph_id, edge_id, reason)
        .await
        .or_not_found("edge")?;
    state
        .hub
        .broadcast(graph_id, WsMessage::EdgeUpserted { edge: edge.clone() });
    touched(state, graph_id).await?;
    Ok(edge)
}

/// Replaces the ontology of a graph. A type that nodes or edges still use
/// cannot be removed (422); turning a relation blocking is refused (409) if
/// its edges would close a cycle. Edges follow their relation's new
/// `blocking` flag.
pub async fn replace_ontology(
    state: &AppState,
    owner: Uuid,
    graph_id: Uuid,
    mut ontology: Ontology,
) -> Result<Graph, AppError> {
    ontology.normalize();
    let mut errors = FieldErrors::default();
    ontology.validate(&mut errors);
    errors.into_result()?;

    let mut tx = state.db.begin().await?;
    repo::graphs::lock(&mut *tx, owner, graph_id)
        .await
        .or_not_found("graph")?;
    let nodes = repo::nodes::list(&mut *tx, graph_id).await?;
    let mut edges = repo::edges::list(&mut *tx, graph_id).await?;
    let mut errors = FieldErrors::default();
    let mut missing: Vec<&str> = nodes
        .iter()
        .map(|n| n.kind.as_str())
        .filter(|k| ontology.node_type(k).is_none())
        .collect();
    missing.sort_unstable();
    missing.dedup();
    for kind in missing {
        errors.add(
            "node_types",
            format!("`{kind}` is still used by nodes of this graph"),
        );
    }
    let mut missing: Vec<&str> = edges
        .iter()
        .map(|e| e.kind.as_str())
        .filter(|k| ontology.relation_type(k).is_none())
        .collect();
    missing.sort_unstable();
    missing.dedup();
    for kind in missing {
        errors.add(
            "relation_types",
            format!("`{kind}` is still used by edges of this graph"),
        );
    }
    errors.into_result()?;

    let mut changed = Vec::new();
    for edge in &mut edges {
        let blocking = ontology
            .relation_type(&edge.kind)
            .is_some_and(|r| r.blocking);
        if edge.blocking != blocking {
            edge.blocking = blocking;
            changed.push(edge.clone());
        }
    }
    let (_, dag) = dependency_graph(&nodes, &edges);
    if !dag.cycles().is_empty() {
        return Err(AppError::Conflict(
            "making this relation blocking would create a dependency cycle".into(),
        ));
    }
    for relation in &ontology.relation_types {
        repo::edges::set_blocking(&mut *tx, graph_id, &relation.key, relation.blocking).await?;
    }
    repo::graphs::set_ontology(&mut *tx, graph_id, &ontology).await?;
    tx.commit().await?;

    state
        .hub
        .broadcast(graph_id, WsMessage::OntologyUpdated { ontology });
    for edge in changed {
        state
            .hub
            .broadcast(graph_id, WsMessage::EdgeUpserted { edge });
    }
    touched(state, graph_id).await?;
    deps::schedule(state, graph_id);
    load_graph(state, owner, graph_id).await
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
