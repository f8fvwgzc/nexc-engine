//! Creates graphs from the built-in starter templates.

use uuid::Uuid;

use super::{deps, editor};
use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::graph::{EdgeKind, EdgeOrigin, Graph, NodeDraft, NodeOrigin, normalize_tags};
use crate::domain::template;
use crate::repo;

/// Instantiates template `template_id` as a new graph owned by `owner`; `topic` (what this
/// instance is about) leads the graph goal so every prompt sees it.
pub async fn instantiate(
    state: &AppState,
    owner: Uuid,
    template_id: &str,
    name: Option<String>,
    topic: Option<String>,
) -> Result<Graph, AppError> {
    let spec = template::find(template_id).ok_or(AppError::NotFound("template"))?;
    let topic = topic.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    let name = name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| template::instance_name(&spec.name, topic.as_deref()));
    let goal = template::instance_goal(&spec.goal, topic.as_deref());
    let mut tx = state.db.begin().await?;
    let graph =
        repo::graphs::create(&mut *tx, owner, name.trim(), &spec.description, &goal).await?;
    let mut ids = Vec::with_capacity(spec.nodes.len());
    for n in &spec.nodes {
        let draft = NodeDraft {
            title: n.title.clone(),
            content: n.content.clone(),
            kind: n.kind,
            tags: normalize_tags(&n.tags),
            x: n.x,
            y: n.y,
            agent_role: n.agent_role.clone(),
            executor: n.executor,
            origin: NodeOrigin::User,
        };
        ids.push(
            repo::nodes::create(&mut *tx, Uuid::now_v7(), graph.id, &draft)
                .await?
                .id,
        );
    }
    for (s, t) in spec.edge_indices() {
        repo::edges::create(
            &mut *tx,
            graph.id,
            ids[s],
            ids[t],
            EdgeKind::DependsOn,
            EdgeOrigin::User,
        )
        .await?;
    }
    tx.commit().await?;
    deps::schedule(state, graph.id);
    editor::load_graph(state, owner, graph.id).await
}
