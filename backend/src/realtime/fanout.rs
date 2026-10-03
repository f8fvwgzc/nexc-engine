//! Cross-instance fan-out of hub messages over PostgreSQL LISTEN/NOTIFY.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use sqlx::postgres::PgListener;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::hub::{Envelope, Hub};
use crate::repo::outbox;

/// A NOTIFY payload: the envelope itself or a reference to a parked one.
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum Notification {
    Parked { origin: Uuid, parked: i64 },
    Inline(Envelope),
}

/// Starts the notifier (local → peers) and listener (peers → local) tasks.
pub fn spawn(hub: Arc<Hub>, db: PgPool, shutdown: CancellationToken) {
    let (tx, rx) = mpsc::channel(4096);
    hub.enable_fanout(tx);
    tokio::spawn(notifier(rx, db.clone(), shutdown.clone()));
    tokio::spawn(listener(hub, db, shutdown));
}

async fn notifier(mut rx: mpsc::Receiver<Envelope>, db: PgPool, shutdown: CancellationToken) {
    loop {
        let envelope = tokio::select! {
            _ = shutdown.cancelled() => return,
            next = rx.recv() => match next { Some(e) => e, None => return },
        };
        if let Err(err) = send(&db, envelope).await {
            tracing::warn!(error = %err, "realtime NOTIFY failed");
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

async fn listener(hub: Arc<Hub>, db: PgPool, shutdown: CancellationToken) {
    while !shutdown.is_cancelled() {
        if let Err(err) = listen(&hub, &db, &shutdown).await {
            tracing::warn!(error = %err, "realtime LISTEN connection lost; reconnecting");
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
    }
}

async fn listen(hub: &Hub, db: &PgPool, shutdown: &CancellationToken) -> anyhow::Result<()> {
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
            Ok(_) => continue,
            Err(err) => {
                tracing::debug!(error = %err, "ignoring malformed realtime notification");
                continue;
            }
        };
        hub.deliver_local(&envelope);
    }
}
