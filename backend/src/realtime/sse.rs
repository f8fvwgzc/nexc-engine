//! The SSE stream of a graph (contract §6).

use std::convert::Infallible;
use std::time::Duration;

use async_stream::stream;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use crate::app::AppState;

/// Interval of `heartbeat` events.
pub const HEARTBEAT: Duration = Duration::from_secs(15);
/// Reconnect delay advertised to clients.
pub const RETRY: Duration = Duration::from_millis(3000);

fn heartbeat(id: u64) -> Event {
    let data = serde_json::json!({ "at": chrono::Utc::now() }).to_string();
    Event::default()
        .id(id.to_string())
        .event("heartbeat")
        .data(data)
}

/// Streams the graph's events to `user_id`: `retry` and a heartbeat first,
/// then every published event with a monotonically increasing id, and a
/// heartbeat every 15 seconds. The stream ends when the account's sessions
/// are ended (suspension, platform role change): at the next event or
/// heartbeat, whichever comes first.
pub fn stream(
    state: AppState,
    graph_id: Uuid,
    user_id: Uuid,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.hub.subscribe_sse(graph_id);
    let admitted_at = state.sessions.floor(user_id);
    let events = stream! {
        yield Ok(Event::default().retry(RETRY));
        yield Ok(heartbeat(state.hub.next_id(graph_id)));
        let mut ticker = tokio::time::interval(HEARTBEAT);
        ticker.tick().await;
        loop {
            let event = tokio::select! {
                _ = ticker.tick() => heartbeat(state.hub.next_id(graph_id)),
                msg = rx.recv() => match msg {
                    Ok(frame) => Event::default().id(frame.id.to_string()).event(&*frame.event).data(&*frame.data),
                    Err(RecvError::Lagged(missed)) => {
                        tracing::warn!(%graph_id, missed, "slow SSE client skipped events");
                        continue;
                    }
                    Err(RecvError::Closed) => break,
                },
            };
            if state.sessions.floor(user_id) > admitted_at {
                break;
            }
            yield Ok(event);
        }
    };
    Sse::new(events).keep_alive(KeepAlive::new().interval(HEARTBEAT))
}
