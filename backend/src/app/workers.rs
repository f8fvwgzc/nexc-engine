//! Background workers: run dispatcher, realtime fan-out, orchestrator
//! heartbeat and housekeeping.

use std::future::Future;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::AppState;
use crate::engine::{knowledge, scheduler};
use crate::{orchestrator, realtime, repo};

const HEARTBEAT_EVERY: Duration = Duration::from_secs(10);
const HOUSEKEEPING_EVERY: Duration = Duration::from_secs(60);
/// How often waiting documents are looked for; a round lasts as long as its documents take.
const INGEST_EVERY: Duration = Duration::from_secs(2);

/// Starts every background worker; they stop when `shutdown` is cancelled.
pub fn spawn(state: &AppState, shutdown: CancellationToken) {
    realtime::fanout::spawn(state.hub.clone(), state.db.clone(), shutdown.clone());
    tokio::spawn(scheduler::dispatcher(state.clone(), shutdown.clone()));
    tokio::spawn(every(
        HEARTBEAT_EVERY,
        shutdown.clone(),
        state.clone(),
        |s| async move {
            orchestrator::heartbeat(&s).await?;
            scheduler::reap_orphans(&s).await
        },
    ));
    tokio::spawn(every(
        INGEST_EVERY,
        shutdown.clone(),
        state.clone(),
        |s| async move { knowledge::work(&s).await },
    ));
    tokio::spawn(every(
        HOUSEKEEPING_EVERY,
        shutdown,
        state.clone(),
        |s| async move { housekeeping(&s).await },
    ));
}

/// Runs `job` every `period` until `shutdown`, logging failures.
async fn every<F, Fut>(period: Duration, shutdown: CancellationToken, state: AppState, job: F)
where
    F: Fn(AppState) -> Fut,
    Fut: Future<Output = anyhow::Result<()>>,
{
    let mut ticker = tokio::time::interval(period);
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = ticker.tick() => {
                if let Err(err) = job(state.clone()).await {
                    tracing::warn!(error = %err, "background job failed");
                }
            }
        }
    }
}

async fn housekeeping(state: &AppState) -> anyhow::Result<()> {
    repo::tickets::purge_expired(&state.db).await?;
    repo::tokens::purge_expired(&state.db).await?;
    repo::outbox::purge(&state.db).await?;
    repo::plans::fail_stale(&state.db).await?;
    state.hub.prune();
    state.limiters.prune();
    Ok(())
}
