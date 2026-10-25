//! Cross-instance fan-out of hub messages and cache invalidations over
//! PostgreSQL LISTEN/NOTIFY.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize};
use sqlx::PgPool;
use sqlx::postgres::{PgListener, PgNotification};
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
    Parked {
        origin: Uuid,
        parked: i64,
    },
    Invalidate {
        origin: Uuid,
        #[serde(deserialize_with = "known_keys")]
        invalidate: Vec<Key>,
    },
    Inline(Envelope),
}

/// The keys of an invalidation this build knows. A kind a later build
/// added is skipped, so that the keys sent with it are still dropped.
fn known_keys<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Key>, D::Error> {
    let keys = Vec::<serde_json::Value>::deserialize(d)?;
    let known = keys
        .into_iter()
        .filter_map(|key| serde_json::from_value::<Key>(key).ok());
    Ok(known.collect())
}

/// The notifications a batch of invalidations goes out in. A build before
/// this one cannot read a notification that holds a kind it lacks, and
/// drops all of it; during a rolling update such a replica should lose
/// only the keys it does not keep anyway, not the policies, guardrails or
/// vocabularies sent with them. So the newer kinds go apart. (From this
/// build on, a replica skips the kinds it lacks: see [`known_keys`].)
fn batches(keys: Vec<Key>) -> impl Iterator<Item = Vec<Key>> {
    let (newer, known): (Vec<Key>, Vec<Key>) = keys.into_iter().partition(is_newer);
    [known, newer].into_iter().filter(|keys| !keys.is_empty())
}

/// Whether builds before this one lack `key`'s kind (an onto-intelli
/// tenant's reads, `ontology::cached`).
fn is_newer(key: &Key) -> bool {
    matches!(key, Key::Ontology(_))
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
                    // Whatever else is waiting goes along, in one notification
                    // per batch an older replica can read.
                    let mut waiting = vec![first];
                    while waiting.len() < 64 {
                        match keys.try_recv() {
                            Ok(key) => waiting.push(key),
                            Err(_) => break,
                        }
                    }
                    for invalidate in batches(waiting) {
                        let payload = Notification::Invalidate { origin: instance, invalidate };
                        let result = match serde_json::to_string(&payload) {
                            Ok(payload) => outbox::notify(&db, &payload).await,
                            Err(err) => Err(sqlx::Error::Encode(Box::new(err))),
                        };
                        if let Err(err) = result {
                            tracing::warn!(error = %err, "cache invalidation NOTIFY failed");
                        }
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
    // Whether invalidations may have been sent while no LISTEN was open.
    let mut gap = false;
    while !shutdown.is_cancelled() {
        if let Err(err) = listen(&hub, &caches, &db, &shutdown, gap).await {
            gap = true;
            tracing::warn!(error = %err, "realtime LISTEN connection lost; reconnecting");
            tokio::select! {
                _ = shutdown.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
    }
}

/// What the other replicas sent while this one was not listening never
/// arrives: every entry kept here is dropped, once LISTEN is open again, so
/// nothing they invalidated meanwhile is served until its TTL runs out.
fn listening_again(caches: &Caches) {
    caches.drop_all_local();
    tracing::info!("realtime LISTEN connection back; every cached entry dropped");
}

/// How long the listener waits for a notification before it asks whether
/// its connection still answers. A NAT or load balancer that forgets an idle
/// connection sends no reset, and a connection that only reads would wait
/// for the next notification forever.
pub const QUIET: Duration = Duration::from_secs(30);

/// How long the database has to answer that question.
pub const ANSWER: Duration = Duration::from_secs(10);

/// The next notification, or an error once the connection is gone: it
/// dropped, or it was quiet for `quiet` and then did not answer a `SELECT 1`
/// within `answer`. The caller opens a new LISTEN and drops its caches.
pub async fn next_notification(
    listener: &mut PgListener,
    quiet: Duration,
    answer: Duration,
) -> anyhow::Result<PgNotification> {
    loop {
        match tokio::time::timeout(quiet, listener.try_recv()).await {
            Ok(received) => {
                return received?.ok_or_else(|| anyhow::anyhow!("the LISTEN connection dropped"));
            }
            Err(_) => {
                // Notifications that come during the query are kept for the
                // next read.
                let ping = sqlx::query("SELECT 1").execute(&mut *listener);
                match tokio::time::timeout(answer, ping).await {
                    Ok(answered) => {
                        answered?;
                    }
                    Err(_) => anyhow::bail!("the LISTEN connection gave no answer in {answer:?}"),
                }
            }
        }
    }
}

async fn listen(
    hub: &Hub,
    caches: &Caches,
    db: &PgPool,
    shutdown: &CancellationToken,
    gap: bool,
) -> anyhow::Result<()> {
    let mut listener = PgListener::connect_with(db).await?;
    // A dropped connection ends this call instead of being replaced inside
    // `try_recv`, where a timeout could cut the reconnect short and lose the
    // drop: the caller then reconnects and drops the caches, once.
    listener.eager_reconnect(false);
    listener.listen(outbox::CHANNEL).await?;
    if gap {
        listening_again(caches);
    }
    loop {
        let notification = tokio::select! {
            _ = shutdown.cancelled() => return Ok(()),
            n = next_notification(&mut listener, QUIET, ANSWER) => n?,
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

    #[test]
    fn a_kind_older_replicas_lack_never_takes_others_down_with_it() {
        let w = Uuid::now_v7();
        let onto = Key::Ontology("ws_1".into());
        let waiting = vec![Key::Vocabulary(w), onto.clone(), Key::Agents];
        let sent: Vec<Vec<Key>> = batches(waiting).collect();
        let known = vec![Key::Vocabulary(w), Key::Agents];
        assert_eq!(sent, vec![known, vec![onto.clone()]]);
        let alone: Vec<Vec<Key>> = batches(vec![onto.clone()]).collect();
        assert_eq!(alone, vec![vec![onto]]);

        // A kind a later build added is skipped; the keys sent with it are still read.
        let later = serde_json::json!({
            "origin": w,
            "invalidate": [{"kind": "from_a_later_build", "id": 1}, Key::Guardrails(w)],
        });
        assert!(matches!(
            serde_json::from_str::<Notification>(&later.to_string()).unwrap(),
            Notification::Invalidate { invalidate, .. } if invalidate == [Key::Guardrails(w)]
        ));
    }
}
