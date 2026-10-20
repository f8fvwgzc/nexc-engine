//! Keeping evidence: deleting a graph archives it with its runs' evidence,
//! and the retention purge is the one way that evidence leaves, audited
//! (docs/AGENTSECOPS.md §12).

use uuid::Uuid;

use crate::app::AppState;
use crate::domain::AppError;
use crate::domain::audit::AuditAction;
use crate::repo::{self, audit::Subject};
use crate::security::signing::{self, Signer};

/// Deleted graphs removed for good in one sweep at most.
const PURGE_BATCH: i64 = 20;

/// Deletes a graph that `user` may work on: archives it, stops its runs,
/// unlinks its issues, and writes `graph_deleted` to its workspace's audit
/// log, in one transaction. Its runs, their spans, seals and payloads stay
/// until the retention purge.
pub async fn delete_graph(state: &AppState, user: Uuid, gid: Uuid) -> Result<(), AppError> {
    let mut tx = state.db.begin().await?;
    let Some((name, workspace)) = repo::graphs::archive(&mut *tx, user, gid).await? else {
        return Err(AppError::NotFound("graph"));
    };
    let running = repo::runs::stop_for_deleted_graph(&mut tx, gid).await?;
    // As when the graph was removed outright: the issues it executed stop pointing at it.
    sqlx::query("UPDATE issues SET graph_id = NULL WHERE graph_id = $1")
        .bind(gid)
        .execute(&mut *tx)
        .await?;
    let (runs, sealed) = repo::runs::count_sealed(&mut *tx, gid).await?;
    if let Some(workspace) = workspace {
        let mut detail = format!(
            "graph {gid}: {runs} run(s) kept with their evidence ({sealed} sealed) until the retention purge"
        );
        if !running.is_empty() {
            detail.push_str(&format!("; {} running run(s) asked to stop", running.len()));
        }
        repo::audit::record(
            &mut *tx,
            workspace,
            user,
            AuditAction::GraphDeleted,
            Subject::Text(&name),
            &detail,
        )
        .await?;
    }
    tx.commit().await?;
    for run in running {
        state.engine.cancel_local(run);
    }
    if let Some(workspace) = workspace {
        // The graph's memories are no longer anyone's to read.
        state.memories.invalidate(workspace);
    }
    tracing::info!(graph_id = %gid, runs, sealed, "graph deleted; its runs' evidence is kept");
    Ok(())
}

/// The retention purge: recorded payloads past their workspace's retention,
/// then deleted graphs whose runs are all past it (never sooner than
/// `repo::evidence::EVIDENCE_DAYS_MIN` days), each removal written to the
/// workspace's audit log in the transaction that makes it.
pub async fn purge_expired(state: &AppState) -> anyhow::Result<()> {
    let payloads = repo::evidence::purge_payloads(&state.db).await?;
    if payloads > 0 {
        tracing::info!(payloads, "recorded payloads past their retention deleted");
    }
    for graph in repo::evidence::expired_graphs(&state.db, PURGE_BATCH).await? {
        // One graph that cannot be removed (a statement timeout on a huge cascade) does not
        // hold up the others.
        if let Err(err) = remove_expired(state, &graph).await {
            tracing::warn!(graph_id = %graph.id, error = %err, "deleted graph not purged; tried again later");
        }
    }
    Ok(())
}

/// Removes one deleted graph past its retention, with its runs and their
/// evidence, and says so in its workspace's audit log, in one transaction.
async fn remove_expired(
    state: &AppState,
    graph: &repo::evidence::ExpiredGraph,
) -> anyhow::Result<()> {
    let runs: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM runs WHERE graph_id = $1")
        .bind(graph.id)
        .fetch_all(&state.db)
        .await?;
    let mut tx = state.db.begin().await?;
    repo::evidence::allow_purge(&mut tx).await?;
    let removed = repo::evidence::graph_contents(&mut *tx, graph.id).await?;
    if !repo::evidence::remove_graph(&mut tx, graph.id).await? {
        // Another replica removed it first.
        return Ok(());
    }
    if let Some(workspace) = graph.workspace_id {
        let detail = format!(
            "graph {} deleted {}: {} run(s), {} span(s) and {} recorded payload(s) removed after {} day(s) of retention",
            graph.id,
            graph.deleted_at.format("%Y-%m-%d"),
            removed.runs,
            removed.spans,
            removed.payloads,
            graph.retention_days
        );
        repo::audit::record_system(
            &mut *tx,
            workspace,
            AuditAction::EvidencePurged,
            &graph.name,
            &detail,
        )
        .await?;
    }
    tx.commit().await?;
    super::artifacts::remove_runs(state, &runs).await;
    tracing::info!(graph_id = %graph.id, runs = removed.runs, spans = removed.spans,
        "deleted graph removed by the retention purge");
    Ok(())
}

/// Checks the published keys against the seals on record, at start: a seal
/// signed by a key that is no longer published (a rotation that left the old
/// public key out of `NEXC_SIGNING_KEYS_RETIRED`) no longer verifies, which is
/// said loudly; a retired entry that is in fact the private seed of a key
/// that signed seals would be published to everyone, so the backend refuses
/// to start.
pub async fn check_signing_keys(state: &AppState) -> anyhow::Result<()> {
    let kids: Vec<(String, i64)> = sqlx::query_as(
        "SELECT seal_kid, count(*) FROM runs WHERE seal_kid IS NOT NULL GROUP BY seal_kid",
    )
    .fetch_all(&state.db)
    .await?;
    let keys = &state.signing_keys;
    for retired in &state.settings.signing_keys_retired {
        let as_seed = Signer::from_seed(retired)?;
        let signs_seals = kids.iter().any(|(kid, _)| kid == as_seed.kid());
        anyhow::ensure!(
            !signs_seals || kids.iter().any(|(kid, _)| *kid == signing::kid(retired)),
            "NEXC_SIGNING_KEYS_RETIRED holds a private seed (the key {} signed seals with it): \
             give its public key instead, from /.well-known/jwks.json",
            as_seed.kid()
        );
    }
    for (kid, runs) in &kids {
        if keys.public_key(kid).is_none() {
            tracing::warn!(%kid, runs,
                "seals were signed with a key that is not published: add its public key to NEXC_SIGNING_KEYS_RETIRED, or they no longer verify");
        }
    }
    Ok(())
}
