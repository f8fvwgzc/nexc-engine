//! Typed realtime messages: SSE events (contract §6) and WebSocket
//! messages (contract §7).

use serde::Serialize;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::domain::graph::{EdgeSuggestion, GraphEdge, GraphNode, GraphSummary, NodeStatus};
use crate::domain::ontology::Ontology;
use crate::domain::plan::{Plan, ProposedEdge, ProposedNode};
use crate::domain::run::{Artifact, Run};

/// `node.status` payload.
#[derive(Debug, Clone, Serialize, ToSchema)]
pub struct NodeStatusEvent {
    pub run_id: Uuid,
    pub node_id: Uuid,
    pub status: NodeStatus,
    pub attempt: i32,
    pub error: Option<String>,
    pub cached: bool,
}

/// Severity of a `node.log` line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// One server-sent event (`data:` payload). The variant determines the SSE `event:` name.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(untagged)]
pub enum SseEvent {
    PlanStarted {
        plan_id: Uuid,
    },
    PlanNode {
        plan_id: Uuid,
        node: ProposedNode,
    },
    PlanEdge {
        plan_id: Uuid,
        edge: ProposedEdge,
    },
    PlanReady {
        plan: Plan,
    },
    PlanFailed {
        plan_id: Uuid,
        error: String,
    },
    RunStarted {
        run: Run,
    },
    NodeStatus(NodeStatusEvent),
    NodeOutput {
        run_id: Uuid,
        node_id: Uuid,
        delta: String,
    },
    NodeLog {
        run_id: Uuid,
        node_id: Uuid,
        level: LogLevel,
        message: String,
    },
    NodeTokens {
        run_id: Uuid,
        node_id: Uuid,
        tokens_in: i64,
        tokens_out: i64,
    },
    ArtifactCreated {
        artifact: Artifact,
    },
    RunFinished {
        run: Run,
    },
}

impl SseEvent {
    /// The SSE `event:` field.
    pub fn name(&self) -> &'static str {
        match self {
            SseEvent::PlanStarted { .. } => "plan.started",
            SseEvent::PlanNode { .. } => "plan.node",
            SseEvent::PlanEdge { .. } => "plan.edge",
            SseEvent::PlanReady { .. } => "plan.ready",
            SseEvent::PlanFailed { .. } => "plan.failed",
            SseEvent::RunStarted { .. } => "run.started",
            SseEvent::NodeStatus(_) => "node.status",
            SseEvent::NodeOutput { .. } => "node.output",
            SseEvent::NodeLog { .. } => "node.log",
            SseEvent::NodeTokens { .. } => "node.tokens",
            SseEvent::ArtifactCreated { .. } => "artifact.created",
            SseEvent::RunFinished { .. } => "run.finished",
        }
    }
}

/// A cursor position on the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, serde::Deserialize, ToSchema)]
pub struct Cursor {
    pub x: f64,
    pub y: f64,
}

/// A server → client WebSocket message.
#[derive(Debug, Clone, Serialize, ToSchema)]
#[serde(tag = "type")]
pub enum WsMessage {
    #[serde(rename = "node.upserted")]
    NodeUpserted { node: GraphNode },
    #[serde(rename = "node.deleted")]
    NodeDeleted { node_id: Uuid },
    #[serde(rename = "edge.upserted")]
    EdgeUpserted { edge: GraphEdge },
    #[serde(rename = "edge.deleted")]
    EdgeDeleted { edge_id: Uuid },
    #[serde(rename = "graph.updated")]
    GraphUpdated { graph: GraphSummary },
    #[serde(rename = "ontology.updated")]
    OntologyUpdated { ontology: Ontology },
    #[serde(rename = "suggestions")]
    Suggestions { items: Vec<EdgeSuggestion> },
    #[serde(rename = "presence")]
    Presence {
        user_id: Uuid,
        name: String,
        cursor: Option<Cursor>,
    },
    #[serde(rename = "pong")]
    Pong,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_shapes() {
        let id = Uuid::nil();
        let e = SseEvent::NodeOutput {
            run_id: id,
            node_id: id,
            delta: "hi".into(),
        };
        assert_eq!(e.name(), "node.output");
        assert_eq!(serde_json::to_value(&e).unwrap()["delta"], "hi");
        let w = serde_json::to_value(WsMessage::EdgeDeleted { edge_id: id }).unwrap();
        assert_eq!(w["type"], "edge.deleted");
        assert_eq!(
            serde_json::to_value(WsMessage::Pong).unwrap(),
            serde_json::json!({"type": "pong"})
        );
        let p = serde_json::to_value(WsMessage::Presence {
            user_id: id,
            name: "a".into(),
            cursor: None,
        })
        .unwrap();
        assert!(p["cursor"].is_null());
    }
}
