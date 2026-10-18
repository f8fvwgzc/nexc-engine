//! Background workers: run dispatcher, realtime fan-out, orchestrator
//! heartbeat and housekeeping.

use std::future::Future;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::AppState;
use crate::engine::{knowledge, scheduler};
use crate::ontology::export;
use crate::{alerting, firewall, orchestrator, otlp, realtime, repo};

const HEARTBEAT_EVERY: Duration = Duration::from_secs(10);
const HOUSEKEEPING_EVERY: Duration = Duration::from_secs(60);
/// How soon a session ended on another server stops working on this one.
const SESSIONS_EVERY: Duration = Duration::from_secs(10);
/// How often waiting documents are looked for; a round lasts as long as its documents take.
const INGEST_EVERY: Duration = Duration::from_secs(2);
/// How often due issues are looked for, to remind their assignees.
const DUE_EVERY: Duration = Duration::from_secs(900);
/// How often workspaces' memory forgetting policies are applied.
const FORGET_EVERY: Duration = Duration::from_secs(3600);

/// Starts every background worker; they stop when `shutdown` is cancelled.
pub fn spawn(state: &AppState, shutdown: CancellationToken) {
    realtime::fanout::spawn(state.hub.clone(), state.db.clone(), shutdown.clone());
    tokio::spawn(scheduler::dispatcher(state.clone(), shutdown.clone()));
    if export::spawn(state, shutdown.clone()) {
        tracing::info!(
            every_secs = export::TICK.as_secs(),
            "the audit's facts are exported to onto-intelli"
        );
    }
    if otlp::spawn(state, shutdown.clone()) {
        tracing::info!(
            every_secs = otlp::TICK.as_secs(),
            endpoint = state.settings.otlp_endpoint.as_deref().unwrap_or_default(),
            "the trace is exported as OpenTelemetry spans"
        );
    }
    tokio::spawn(firewall::run(state.clone(), shutdown.clone()));
    alerting::spawn(state, shutdown.clone());
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
        SESSIONS_EVERY,
        shutdown.clone(),
        state.clone(),
        |s| async move {
            s.sessions.sync(&s.db, s.settings.access_ttl).await?;
            Ok(())
        },
    ));
    tokio::spawn(every(
        INGEST_EVERY,
        shutdown.clone(),
        state.clone(),
        |s| async move { knowledge::work(&s).await },
    ));
    tokio::spawn(every(
        DUE_EVERY,
        shutdown.clone(),
        state.clone(),
        |s| async move {
            repo::issues::remind_due(&s.db).await?;
            Ok(())
        },
    ));
    tokio::spawn(every(
        FORGET_EVERY,
        shutdown.clone(),
        state.clone(),
        |s| async move {
            crate::memory::forget(&s.memories, &s.db).await?;
            Ok(())
        },
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
    repo::resets::purge_expired(&state.db).await?;
    repo::account_events::purge(&state.db).await?;
    repo::outbox::purge(&state.db).await?;
    repo::plans::fail_stale(&state.db).await?;
    let purged = repo::span_payloads::purge_expired(&state.db).await?;
    if purged > 0 {
        tracing::info!(purged, "recorded payloads past their retention deleted");
    }
    export::purge_when_off(state).await?;
    state
        .firewall_sessions
        .seal_idle(state, firewall::SESSION_IDLE)
        .await;
    state.hub.prune();
    state.limiters.prune();
    state.engine.prune_dispatches();
    state.gateway_registry.prune();
    Ok(())
}
