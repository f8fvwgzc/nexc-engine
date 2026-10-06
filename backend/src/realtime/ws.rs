//! The collaborative WebSocket of a graph (contract §7).

use std::collections::HashMap;
use std::time::Duration;

use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use super::events::{Cursor, WsMessage};
use crate::app::AppState;
use crate::domain::validation::{FieldErrors, check_coordinate};
use crate::dsa::token_bucket::TokenBucket;
use crate::engine::editor;

/// Largest accepted client frame.
pub const MAX_FRAME_BYTES: usize = 16 * 1024;
/// Client frames allowed per second per socket (burst of twice that);
/// excess frames are dropped.
const FRAMES_PER_SEC: u32 = 30;
/// Node moves are persisted at most this often per socket.
const MOVE_FLUSH: Duration = Duration::from_millis(100);
/// How often an open socket checks that its account's sessions were not ended.
const SESSION_CHECK: Duration = Duration::from_secs(5);

/// A client → server message.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ClientMessage {
    #[serde(rename = "node.move")]
    NodeMove { node_id: Uuid, x: f64, y: f64 },
    #[serde(rename = "presence")]
    Presence { cursor: Option<Cursor> },
    #[serde(rename = "ping")]
    Ping,
}

/// Who is connected.
#[derive(Debug, Clone)]
pub struct Peer {
    pub user_id: Uuid,
    pub name: String,
    pub graph_id: Uuid,
}

fn text(message: &WsMessage) -> Message {
    Message::Text(Utf8Bytes::from(
        serde_json::to_string(message).expect("messages serialise"),
    ))
}

/// Runs one socket until it closes.
pub async fn session(state: AppState, socket: WebSocket, peer: Peer) {
    let (mut sink, mut source) = socket.split();
    let mut hub = state.hub.subscribe_ws(peer.graph_id);
    let mut moves: HashMap<Uuid, (f64, f64)> = HashMap::new();
    let mut flush = tokio::time::interval(MOVE_FLUSH);
    let mut budget = TokenBucket::new(FRAMES_PER_SEC * 2, f64::from(FRAMES_PER_SEC));
    // The socket closes when the account's sessions are ended (suspension, platform role change).
    let admitted_at = state.sessions.floor(peer.user_id);
    let mut session_check = tokio::time::interval(SESSION_CHECK);
    loop {
        tokio::select! {
            _ = session_check.tick() => {
                if state.sessions.floor(peer.user_id) > admitted_at {
                    break;
                }
            }
            broadcast = hub.recv() => match broadcast {
                Ok(json) => {
                    if sink.send(Message::Text(Utf8Bytes::from(&*json))).await.is_err() {
                        break;
                    }
                }
                Err(RecvError::Lagged(missed)) => tracing::warn!(graph_id = %peer.graph_id, missed, "slow socket skipped messages"),
                Err(RecvError::Closed) => break,
            },
            frame = source.next() => match frame {
                Some(Ok(Message::Text(t))) if budget.try_acquire().is_ok() => {
                    if let Some(reply) = handle(&state, &peer, t.as_str(), &mut moves)
                        && sink.send(text(&reply)).await.is_err()
                    {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                // Binary frames, control frames and frames over the budget are ignored.
                Some(Ok(_)) => {}
            },
            _ = flush.tick(), if !moves.is_empty() => flush_moves(&state, &peer, &mut moves).await,
        }
    }
    flush_moves(&state, &peer, &mut moves).await;
    state.hub.broadcast(
        peer.graph_id,
        WsMessage::Presence {
            user_id: peer.user_id,
            name: peer.name,
            cursor: None,
        },
    );
}

/// Handles one client frame; returns a direct reply if any.
fn handle(
    state: &AppState,
    peer: &Peer,
    frame: &str,
    moves: &mut HashMap<Uuid, (f64, f64)>,
) -> Option<WsMessage> {
    if frame.len() > MAX_FRAME_BYTES {
        return None;
    }
    match serde_json::from_str::<ClientMessage>(frame).ok()? {
        ClientMessage::Ping => Some(WsMessage::Pong),
        ClientMessage::NodeMove { node_id, x, y } => {
            let mut errors = FieldErrors::default();
            check_coordinate(&mut errors, "x", x);
            check_coordinate(&mut errors, "y", y);
            if errors.is_empty() {
                moves.insert(node_id, (x, y));
            }
            None
        }
        ClientMessage::Presence { cursor } => {
            let cursor = cursor.filter(|c| c.x.is_finite() && c.y.is_finite());
            let message = WsMessage::Presence {
                user_id: peer.user_id,
                name: peer.name.clone(),
                cursor,
            };
            state.hub.broadcast(peer.graph_id, message);
            None
        }
    }
}

async fn flush_moves(state: &AppState, peer: &Peer, moves: &mut HashMap<Uuid, (f64, f64)>) {
    for (node_id, (x, y)) in moves.drain() {
        if let Err(err) = editor::move_node(state, peer.graph_id, node_id, x, y).await {
            tracing::warn!(%node_id, error = %err, "cannot persist node move");
        }
    }
}
