//! Cross-instance fan-out of hub messages and cache invalidations over
//! PostgreSQL LISTEN/NOTIFY.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::hub::{Envelope, Hub};
use crate::app::caches::{Caches, Key};
use crate::repo::outbox;

/// A NOTIFY payload: the envelope itself, a reference to a parked one, or
/// the cache entries a write made stale.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum Notification {
    Parked { origin: Uuid, parked: i64 },
    Invalidate { origin: Uuid, invalidate: Vec<Key> },
    Inline(Envelope),
}

/// Starts the notifier (local → peers) and listener (peers → local) tasks.
pub fn spawn(hub: Arc<Hub>, caches: Arc<Caches>, db: PgPool, shutdown: CancellationToken) {
    let (tx, rx) = mpsc::channel(4096);
    hub.enable_fanout(tx);
    let (keys_tx, keys_rx) = mpsc::channel(4096);
    caches.enable_fanout(keys_tx);
    tokio::spawn(notifier(
        rx,
        keys_rx,
        hub.instance(),
        db.clone(),
        shutdown.clone(),
    ));
    tokio::spawn(listener(hub, caches, db, shutdown));
}

async fn notifier(
    mut rx: mpsc::Receiver<Envelope>,
    mut keys: mpsc::Receiver<Key>,
    instance: Uuid,
    db: PgPool,
    shutdown: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            next = rx.recv() => match next {
                Some(envelope) => {
                    if let Err(err) = send(&db, envelope).await {
                        tracing::warn!(error = %err, "realtime NOTIFY failed");
                    }
                }
                None => return,
            },
            next = keys.recv() => match next {
                Some(first) => {
                    // Whatever else is waiting goes in the same notification.
                    let mut invalidate = vec![first];
                    while invalidate.len() < 64 {
                        match keys.try_recv() {
                            Ok(key) => invalidate.push(key),
                            Err(_) => break,
                        }
                    }
                    let payload = Notification::Invalidate { origin: instance, invalidate };
                    let result = match serde_json::to_string(&payload) {
                        Ok(payload) => outbox::notify(&db, &payload).await,
                        Err(err) => Err(sqlx::Error::Encode(Box::new(err))),
                    };
                    if let Err(err) = result {
                        tracing::warn!(error = %err, "cache invalidation NOTIFY failed");
                    }
                }
                None => return,
            },
        }
    }
}

async fn send(db: &PgPool, envelope: Envelope) -> anyhow::Result<()> {
    let inline = serde_json::to_string(&Notification::Inline(envelope.clone()))?;
    let payload = if inline.len() <= outbox::INLINE_LIMIT {
        inline
    } else {
        let parked = outbox::park(db, &serde_json::to_value(&envelope)?).await?;
        serde_json::to_string(&Notification::Parked {
            origin: envelope.origin,
            parked,
        })?
    };
    outbox::notify(db, &payload).await?;
    Ok(())
}

async fn listener(hub: Arc<Hub>, caches: Arc<Caches>, db: PgPool, shutdown: CancellationToken) {
    while !shutdown.is_cancelled() {
        if let Err(err) = listen(&hub, &caches, &db, &shutdown).await {
            tracing::warn!(error = %err, "realtime LISTEN connection lost; reconnecting");
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
    }
}

async fn listen(
    hub: &Hub,
    caches: &Caches,
    db: &PgPool,
    shutdown: &CancellationToken,
) -> anyhow::Result<()> {
    let mut listener = PgListener::connect_with(db).await?;
    listener.listen(outbox::CHANNEL).await?;
    loop {
        let notification = tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            n = listener.recv() => n?,
        };
        let envelope = match serde_json::from_str::<Notification>(notification.payload()) {
            Ok(Notification::Inline(e)) if e.origin != hub.instance() => e,
            Ok(Notification::Parked { origin, parked }) if origin != hub.instance() => {
                match outbox::fetch(db, parked).await? {
                    Some(v) => serde_json::from_value(v)?,
                    None => continue,
                }
            }
            Ok(Notification::Invalidate { origin, invalidate }) => {
                if origin != hub.instance() {
                    for key in &invalidate {
                        caches.drop_local(key);
                    }
                }
                continue;
            }
            Ok(_) => continue,
            Err(err) => {
                tracing::debug!(error = %err, "ignoring malformed realtime notification");
                continue;
            }
        };
        hub.deliver_local(&envelope);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notifications_tell_apart_on_the_wire() {
        let w = Uuid::now_v7();
        let invalidate = serde_json::to_string(&Notification::Invalidate {
            origin: w,
            invalidate: vec![Key::Guardrails(w), Key::Agents],
        })
        .unwrap();
        assert!(matches!(
            serde_json::from_str::<Notification>(&invalidate).unwrap(),
            Notification::Invalidate { invalidate, .. } if invalidate.len() == 2
        ));
        let parked = serde_json::to_string(&Notification::Parked {
            origin: w,
            parked: 7,
        })
        .unwrap();
        assert!(matches!(
            serde_json::from_str::<Notification>(&parked).unwrap(),
            Notification::Parked { parked: 7, .. }
        ));
        let inline = serde_json::to_string(&Notification::Inline(Envelope {
            origin: w,
            graph_id: w,
            event: Some("run.started".into()),
            data: serde_json::json!({"run": {}}),
        }))
        .unwrap();
        assert!(matches!(
            serde_json::from_str::<Notification>(&inline).unwrap(),
            Notification::Inline(_)
        ));
    }
}
