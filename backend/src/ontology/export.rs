//! The export worker: what the tables queued for onto-intelli (spans
//! written, runs finished, agents changed) becomes facts and goes out as one
//! ingest stream per workspace and pass; a workspace seen for the first time
//! is backfilled from what it already has. Queue rows are claimed with
//! `SKIP LOCKED` and deleted once onto-intelli acknowledged the stream, so
//! every span is exported at least once, instances share the work, and a
//! crash or an unreachable server costs nothing but a retry.

use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::facts::{Batch, ReadLabels};
use super::{OntoError, OntologyBackend, workspace_name};
use crate::app::AppState;
use crate::domain::trace::SpanKind;
use crate::repo::onto_export::ExportSpan;
use crate::repo::{self, onto_export as cursors};

/// How often the queue is looked at.
pub const TICK: Duration = Duration::from_secs(2);
/// Queue rows claimed per workspace and pass.
pub const CLAIM_BATCH: i64 = 1000;
/// Spans per backfill stream.
pub const BACKFILL_BATCH: i64 = 1000;
/// Runs per backfill stream.
const RUNS_BATCH: usize = 500;
/// Backfill streams per workspace and pass, so one large workspace does not
/// hold the others up.
const BACKFILL_STREAMS_PER_PASS: usize = 20;

/// What one pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    /// Workspaces with something to export.
    pub workspaces: usize,
    pub spans: usize,
    pub records: usize,
    /// Workspaces whose export failed (left for the next pass).
    pub failed: usize,
}

impl Progress {
    fn add(&mut self, other: Progress) {
        self.workspaces += other.workspaces;
        self.spans += other.spans;
        self.records += other.records;
        self.failed += other.failed;
    }
}

/// Starts the worker when an onto-intelli is configured; returns whether it did.
pub fn spawn(state: &AppState, shutdown: CancellationToken) -> bool {
    if state.ontology.is_none() {
        return false;
    }
    tokio::spawn(run(state.clone(), shutdown));
    true
}

async fn run(state: AppState, shutdown: CancellationToken) {
    let mut ticker = tokio::time::interval(TICK);
    let mut failing = false;
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = ticker.tick() => {
                match pass(&state).await {
                    Ok(progress) => {
                        if progress.failed > 0 && !failing {
                            tracing::warn!(?progress, "onto-intelli export failed; retrying every {}s", TICK.as_secs());
                        } else if progress.failed == 0 && failing {
                            tracing::info!("onto-intelli export resumed");
                        }
                        failing = progress.failed > 0;
                        if progress.records > 0 {
                            tracing::debug!(?progress, "onto-intelli export pass");
                        }
                    }
                    Err(err) => tracing::warn!(error = %err, "onto-intelli export pass failed"),
                }
            }
        }
    }
}

/// One pass: every workspace with something queued or not yet backfilled.
pub async fn pass(state: &AppState) -> anyhow::Result<Progress> {
    let mut progress = Progress::default();
    if state.ontology.is_none() {
        return Ok(progress);
    }
    for workspace_id in cursors::workspaces_behind(&state.db).await? {
        progress.workspaces += 1;
        match export_workspace(state, workspace_id).await {
            Ok(done) => progress.add(done),
            Err(err) => {
                progress.failed += 1;
                tracing::debug!(%workspace_id, error = %err, "workspace export failed");
                if let Err(note) =
                    cursors::note_error(&state.db, workspace_id, &err.to_string()).await
                {
                    tracing::debug!(error = %note, "export error not noted");
                }
            }
        }
    }
    Ok(progress)
}

/// Exports one workspace: its backfill first if it never had one, then
/// what is queued for it.
pub async fn export_workspace(state: &AppState, workspace_id: Uuid) -> anyhow::Result<Progress> {
    let backend = state
        .ontology
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("onto-intelli is not configured"))?;
    let workspace = workspace_name(&state.settings.onto_workspace_prefix, workspace_id);
    cursors::ensure_cursor(&state.db, workspace_id).await?;
    let mut progress = Progress::default();
    let cursor = cursors::cursor(&state.db, workspace_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("the workspace is gone"))?;
    if cursor.backfilled_at.is_none() {
        let before = match cursor.backfill_before {
            Some(before) => before,
            None => {
                let boundary = cursors::backfill_boundary(&state.db, workspace_id).await?;
                cursors::set_backfill_boundary(&state.db, workspace_id, boundary).await?
            }
        };
        let done = backfill(
            state,
            backend.as_ref(),
            workspace_id,
            &workspace,
            cursor.span_seq,
            before,
        )
        .await?;
        progress.add(done);
    }
    progress.add(drain(state, backend.as_ref(), workspace_id, &workspace).await?);
    Ok(progress)
}

/// Everything the workspace had before the queue took over: agents and
/// their policies, runs with their status, then every span before `before`
/// in insertion order. States what is; the events of what happens from now
/// on come through the queue.
async fn backfill(
    state: &AppState,
    backend: &dyn OntologyBackend,
    workspace_id: Uuid,
    workspace: &str,
    mut seq: i64,
    before: i64,
) -> anyhow::Result<Progress> {
    let mut progress = Progress::default();
    if seq == 0 {
        let agents = cursors::agents_of(&state.db, workspace_id).await?;
        let mut batch = Batch::new(workspace);
        for agent in &agents {
            batch.agent(agent, &agents, false);
        }
        progress.records += send(backend, workspace, batch).await?;
        let runs = cursors::runs_of(&state.db, workspace_id).await?;
        for chunk in runs.chunks(RUNS_BATCH) {
            let mut batch = Batch::new(workspace);
            for run in chunk {
                batch.run(run, run.finished_at.is_some() && !run.queued);
            }
            progress.records += send(backend, workspace, batch).await?;
        }
    }
    for _ in 0..BACKFILL_STREAMS_PER_PASS {
        let spans =
            cursors::spans_after(&state.db, workspace_id, seq, before, BACKFILL_BATCH).await?;
        let Some(last) = spans.last() else {
            cursors::mark_backfilled(&state.db, workspace_id).await?;
            return Ok(progress);
        };
        let last_seq = last.seq;
        let batch = spans_batch(state, workspace, &spans, &[]).await?;
        let spans_in = batch.spans;
        let records = send(backend, workspace, batch).await?;
        cursors::advance_backfill(
            &state.db,
            workspace_id,
            last_seq,
            spans_in as i64,
            records as i64,
        )
        .await?;
        progress.spans += spans_in;
        progress.records += records;
        seq = last_seq;
        if spans.len() < BACKFILL_BATCH as usize {
            cursors::mark_backfilled(&state.db, workspace_id).await?;
            return Ok(progress);
        }
    }
    Ok(progress)
}

/// Claims what is queued for the workspace and streams it, until the queue
/// is empty; a stream that fails leaves its rows queued.
async fn drain(
    state: &AppState,
    backend: &dyn OntologyBackend,
    workspace_id: Uuid,
    workspace: &str,
) -> anyhow::Result<Progress> {
    let mut progress = Progress::default();
    loop {
        let mut tx = state.db.begin().await?;
        let claimed = cursors::claim(&mut tx, workspace_id, CLAIM_BATCH).await?;
        if claimed.ids.is_empty() {
            return Ok(progress);
        }
        let mut batch = Batch::new(workspace);
        if !claimed.agents.is_empty() {
            let all = cursors::agents_of(&state.db, workspace_id).await?;
            for agent in cursors::agents_by_id(&state.db, &claimed.agents).await? {
                batch.agent(&agent, &all, true);
            }
        }
        for run in cursors::runs_by_id(&state.db, &claimed.runs).await? {
            batch.run(&run, run.finished_at.is_some());
        }
        let spans = cursors::spans_by_id(&state.db, &claimed.spans).await?;
        let batch = spans_batch_into(state, batch, &spans, &claimed.runs).await?;
        let spans_in = batch.spans;
        let records = send(backend, workspace, batch).await?;
        cursors::finish(&mut tx, &claimed.ids).await?;
        cursors::count_exported(&mut *tx, workspace_id, spans_in as i64, records as i64).await?;
        tx.commit().await?;
        progress.spans += spans_in;
        progress.records += records;
        if (claimed.ids.len() as i64) < CLAIM_BATCH {
            return Ok(progress);
        }
    }
}

async fn spans_batch(
    state: &AppState,
    workspace: &str,
    spans: &[ExportSpan],
    runs_already: &[Uuid],
) -> anyhow::Result<Batch> {
    spans_batch_into(state, Batch::new(workspace), spans, runs_already).await
}

/// The facts of `spans`: their runs' status and cost, the documents behind
/// what they read, then the spans themselves.
async fn spans_batch_into(
    state: &AppState,
    mut batch: Batch,
    spans: &[ExportSpan],
    runs_already: &[Uuid],
) -> anyhow::Result<Batch> {
    let mut run_ids: Vec<Uuid> = spans
        .iter()
        .map(|s| s.span.run_id)
        .filter(|id| !runs_already.contains(id))
        .collect();
    run_ids.sort_unstable();
    run_ids.dedup();
    let runs: HashMap<Uuid, _> = cursors::runs_by_id(&state.db, &run_ids)
        .await?
        .into_iter()
        .map(|r| (r.id, r))
        .collect();
    let labels = labels_for(state, spans).await?;
    for exported in spans {
        let read = labels.get(&exported.span.id);
        batch.span(
            exported,
            runs.get(&exported.span.run_id),
            read.unwrap_or(&ReadLabels::default()),
        );
    }
    Ok(batch)
}

fn ids_of(attributes: &Value, key: &str) -> Vec<Uuid> {
    attributes
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|s| s.parse().ok())
        .collect()
}

/// The documents and memories each `data_read` span read, with their labels.
async fn labels_for(
    state: &AppState,
    spans: &[ExportSpan],
) -> anyhow::Result<HashMap<Uuid, ReadLabels>> {
    let reads: Vec<&ExportSpan> = spans
        .iter()
        .filter(|s| s.span.kind == SpanKind::DataRead)
        .collect();
    if reads.is_empty() {
        return Ok(HashMap::new());
    }
    let mut chunk_ids: Vec<Uuid> = reads
        .iter()
        .flat_map(|s| ids_of(&s.span.attributes, "document_chunk_ids"))
        .collect();
    chunk_ids.sort_unstable();
    chunk_ids.dedup();
    let mut memory_ids: Vec<Uuid> = reads
        .iter()
        .flat_map(|s| ids_of(&s.span.attributes, "memory_ids"))
        .collect();
    memory_ids.sort_unstable();
    memory_ids.dedup();
    let documents: HashMap<Uuid, (Uuid, String)> =
        cursors::documents_of_chunks(&state.db, &chunk_ids)
            .await?
            .into_iter()
            .map(|(chunk, document, label)| (chunk, (document, label)))
            .collect();
    let memories: HashMap<Uuid, String> = cursors::memory_labels(&state.db, &memory_ids)
        .await?
        .into_iter()
        .collect();
    let mut out = HashMap::with_capacity(reads.len());
    for read in reads {
        let mut labels = ReadLabels::default();
        for chunk in ids_of(&read.span.attributes, "document_chunk_ids") {
            if let Some((document, label)) = documents.get(&chunk)
                && !labels.documents.iter().any(|(d, _)| d == document)
            {
                labels.documents.push((*document, label.clone()));
            }
        }
        for memory in ids_of(&read.span.attributes, "memory_ids") {
            if let Some(label) = memories.get(&memory) {
                labels.memories.push((memory, label.clone()));
            }
        }
        out.insert(read.span.id, labels);
    }
    Ok(out)
}

/// Streams a batch; the number of records that went out.
async fn send(
    backend: &dyn OntologyBackend,
    workspace: &str,
    batch: Batch,
) -> Result<usize, OntoError> {
    if batch.is_empty() {
        return Ok(0);
    }
    let count = batch.records.len();
    backend.ingest(workspace, batch.records).await?;
    Ok(count)
}

/// Nothing consumes the queue while the feature is off: housekeeping empties it.
pub async fn purge_when_off(state: &AppState) -> anyhow::Result<()> {
    if state.ontology.is_none() {
        let purged = repo::onto_export::purge(&state.db).await?;
        if purged > 0 {
            tracing::debug!(
                purged,
                "onto-intelli export queue emptied: no server is configured"
            );
        }
    }
    Ok(())
}
