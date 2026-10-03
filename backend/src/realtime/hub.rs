//! Per-graph broadcast hub.
//!
//! Local subscribers are served from tokio `broadcast` channels (the fast
//! path). When fan-out is enabled every published message is also sent to
//! the other backend replicas through PostgreSQL `NOTIFY`
//! (see [`super::fanout`]); messages received from peers are delivered
//! locally only, so nothing loops.

use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

use super::events::{SseEvent, WsMessage};

const CHANNEL_CAPACITY: usize = 1024;

/// A message ready to be written to an SSE stream.
#[derive(Debug, Clone)]
pub struct SseFrame {
    /// Monotonically increasing per graph (per instance).
    pub id: u64,
    pub event: Arc<str>,
    pub data: Arc<str>,
}

/// Wire form of a hub message exchanged between instances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Envelope {
    pub origin: Uuid,
    pub graph_id: Uuid,
    /// SSE event name, or `None` for a WebSocket message.
    pub event: Option<String>,
    pub data: serde_json::Value,
}

struct GraphChannel {
    sse: broadcast::Sender<SseFrame>,
    ws: broadcast::Sender<Arc<str>>,
    seq: AtomicU64,
}

/// Realtime hub shared by all request handlers and engine tasks.
pub struct Hub {
    instance: Uuid,
    graphs: DashMap<Uuid, Arc<GraphChannel>>,
    fanout: OnceLock<mpsc::Sender<Envelope>>,
}

impl Default for Hub {
    fn default() -> Self {
        Hub {
            instance: Uuid::now_v7(),
            graphs: DashMap::new(),
            fanout: OnceLock::new(),
        }
    }
}

impl Hub {
    /// Identifier of this backend instance.
    pub fn instance(&self) -> Uuid {
        self.instance
    }

    fn channel(&self, graph_id: Uuid) -> Arc<GraphChannel> {
        self.graphs
            .entry(graph_id)
            .or_insert_with(|| {
                Arc::new(GraphChannel {
                    sse: broadcast::channel(CHANNEL_CAPACITY).0,
                    ws: broadcast::channel(CHANNEL_CAPACITY).0,
                    seq: AtomicU64::new(0),
                })
            })
            .clone()
    }

    /// Sends every future message to `tx` for cross-instance fan-out.
    pub fn enable_fanout(&self, tx: mpsc::Sender<Envelope>) {
        let _ = self.fanout.set(tx);
    }

    /// Publishes an SSE event to the graph's subscribers.
    pub fn publish(&self, graph_id: Uuid, event: SseEvent) {
        let data = serde_json::to_value(&event).expect("events serialise");
        self.dispatch(Envelope {
            origin: self.instance,
            graph_id,
            event: Some(event.name().to_owned()),
            data,
        });
    }

    /// Publishes a WebSocket message to the graph's sockets.
    pub fn broadcast(&self, graph_id: Uuid, message: WsMessage) {
        let data = serde_json::to_value(&message).expect("messages serialise");
        self.dispatch(Envelope {
            origin: self.instance,
            graph_id,
            event: None,
            data,
        });
    }

    fn dispatch(&self, envelope: Envelope) {
        self.deliver_local(&envelope);
        if let Some(tx) = self.fanout.get()
            && tx.try_send(envelope).is_err()
        {
            tracing::warn!("realtime fan-out queue full; dropping cross-instance message");
        }
    }

    /// Delivers an envelope to this instance's subscribers only.
    pub fn deliver_local(&self, envelope: &Envelope) {
        let Some(channel) = self.graphs.get(&envelope.graph_id).map(|c| c.clone()) else {
            return;
        };
        let data: Arc<str> = envelope.data.to_string().into();
        match &envelope.event {
            Some(event) => {
                if channel.sse.receiver_count() > 0 {
                    let id = channel.seq.fetch_add(1, Ordering::Relaxed) + 1;
                    let _ = channel.sse.send(SseFrame {
                        id,
                        event: event.as_str().into(),
                        data,
                    });
                }
            }
            None => {
                let _ = channel.ws.send(data);
            }
        }
    }

    /// Subscribes to SSE events of a graph.
    pub fn subscribe_sse(&self, graph_id: Uuid) -> broadcast::Receiver<SseFrame> {
        self.channel(graph_id).sse.subscribe()
    }

    /// Subscribes to WebSocket messages of a graph.
    pub fn subscribe_ws(&self, graph_id: Uuid) -> broadcast::Receiver<Arc<str>> {
        self.channel(graph_id).ws.subscribe()
    }

    /// Next SSE id of a graph (used for heartbeats so ids stay monotonic).
    pub fn next_id(&self, graph_id: Uuid) -> u64 {
        self.channel(graph_id).seq.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Drops channels nobody listens to any more.
    pub fn prune(&self) {
        self.graphs
            .retain(|_, c| c.sse.receiver_count() > 0 || c.ws.receiver_count() > 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn delivers_in_order_with_increasing_ids() {
        let hub = Hub::default();
        let g = Uuid::now_v7();
        let mut rx = hub.subscribe_sse(g);
        hub.publish(g, SseEvent::PlanStarted { plan_id: g });
        hub.publish(
            g,
            SseEvent::PlanFailed {
                plan_id: g,
                error: "x".into(),
            },
        );
        let a = rx.recv().await.unwrap();
        let b = rx.recv().await.unwrap();
        assert_eq!((&*a.event, &*b.event), ("plan.started", "plan.failed"));
        assert!(b.id > a.id);
        assert!(hub.next_id(g) > b.id);
    }

    #[tokio::test]
    async fn fanout_and_pruning() {
        let hub = Hub::default();
        let (tx, mut out) = mpsc::channel(8);
        hub.enable_fanout(tx);
        let g = Uuid::now_v7();
        let mut ws = hub.subscribe_ws(g);
        hub.broadcast(g, WsMessage::Pong);
        assert_eq!(&*ws.recv().await.unwrap(), r#"{"type":"pong"}"#);
        let env = out.recv().await.unwrap();
        assert_eq!(
            (env.origin, env.graph_id, env.event),
            (hub.instance(), g, None)
        );
        drop(ws);
        hub.prune();
        assert!(hub.graphs.is_empty());
    }
}
