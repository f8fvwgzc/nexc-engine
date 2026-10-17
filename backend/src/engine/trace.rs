//! The per-run trace writer. Executors emit span drafts; one task per run
//! gives each its place in the run's hash chain in order of receipt and
//! fans it out at once as a `trace.span` event; a second task writes the
//! chained spans to the database in batches. Nothing here is on the hot
//! path of the work being traced: emitting is a channel send.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::app::AppState;
use crate::domain::trace::{self, GENESIS, Span, SpanDraft};
use crate::realtime::events::SseEvent;
use crate::repo;

/// Spans written in one statement at most.
const BATCH_MAX: usize = 500;
/// Attempts to write a batch before it is given up.
const WRITE_ATTEMPTS: u32 = 3;

struct Pending {
    draft: SpanDraft,
    emitted: Instant,
}

/// Emits spans into one run's chain. Cheap to clone. Emitting never blocks
/// and never fails the work being traced.
#[derive(Clone)]
pub struct TraceHandle {
    tx: mpsc::UnboundedSender<Pending>,
}

impl TraceHandle {
    /// Appends a span to the run's chain (asynchronously).
    pub fn emit(&self, draft: SpanDraft) {
        let pending = Pending {
            draft,
            emitted: Instant::now(),
        };
        if self.tx.send(pending).is_err() {
            tracing::debug!("span emitted after the run's trace was sealed; dropped");
        }
    }

    /// A handle whose spans go nowhere (work that is not part of a run).
    pub fn discard() -> Self {
        let (tx, _) = mpsc::unbounded_channel();
        TraceHandle { tx }
    }
}

/// What sealing a run's trace yields.
#[derive(Debug, Clone, Copy)]
pub struct Sealed {
    /// The chain's last hash; `None` when nothing was traced.
    pub hash: Option<[u8; 32]>,
    /// Spans in the chain.
    pub spans: i64,
}

/// The open trace of one run.
pub struct TraceSink {
    handle: TraceHandle,
    chain: JoinHandle<Sealed>,
}

/// Opens the trace of `run_id`.
pub fn start(state: AppState, run_id: Uuid, graph_id: Uuid) -> TraceSink {
    let (tx, rx) = mpsc::unbounded_channel();
    let (persist_tx, persist_rx) = mpsc::unbounded_channel();
    let persister = tokio::spawn(persist(state.clone(), run_id, persist_rx));
    let chain = tokio::spawn(chain(state, run_id, graph_id, rx, persist_tx, persister));
    TraceSink {
        handle: TraceHandle { tx },
        chain,
    }
}

impl TraceSink {
    /// A handle to emit spans through.
    pub fn handle(&self) -> TraceHandle {
        self.handle.clone()
    }

    /// Closes the chain: every span emitted so far is hashed and written
    /// before this returns. Spans emitted afterwards are dropped.
    pub async fn seal(self) -> Sealed {
        drop(self.handle);
        match self.chain.await {
            Ok(sealed) => sealed,
            Err(err) => {
                tracing::error!(error = %err, "trace writer failed; the run is left unsealed");
                Sealed {
                    hash: None,
                    spans: 0,
                }
            }
        }
    }
}

/// Hashes spans in order of receipt and fans them out.
async fn chain(
    state: AppState,
    run_id: Uuid,
    graph_id: Uuid,
    mut rx: mpsc::UnboundedReceiver<Pending>,
    persist_tx: mpsc::UnboundedSender<Span>,
    persister: JoinHandle<()>,
) -> Sealed {
    let mut prev = GENESIS;
    let mut seq: i32 = 0;
    let mut seen: HashSet<Uuid> = HashSet::new();
    while let Some(Pending { mut draft, emitted }) = rx.recv().await {
        if !seen.insert(draft.id) {
            // A runtime that repeats an id gets a fresh one; the repeat is on record.
            let original = draft.id;
            draft.id = Uuid::now_v7();
            seen.insert(draft.id);
            if let Value::Object(map) = &mut draft.attributes {
                map.insert("duplicate_of".into(), json!(original));
            }
        }
        let span = trace::link(draft, run_id, seq, prev);
        prev = span.hash;
        seq += 1;
        state
            .hub
            .publish(graph_id, SseEvent::TraceSpan { span: span.clone() });
        state.metrics.span_fanout.observe(emitted.elapsed());
        state.metrics.spans.add(&[("kind", span.kind.as_str())], 1);
        if persist_tx.send(span).is_err() {
            tracing::error!(%run_id, "trace persister stopped; spans are no longer written");
        }
    }
    drop(persist_tx);
    if let Err(err) = persister.await {
        tracing::error!(%run_id, error = %err, "trace persister failed");
    }
    Sealed {
        hash: (seq > 0).then_some(prev),
        spans: seq as i64,
    }
}

/// Writes chained spans in batches: whatever has arrived while the previous
/// batch was being written goes into the next statement.
async fn persist(state: AppState, run_id: Uuid, mut rx: mpsc::UnboundedReceiver<Span>) {
    let mut batch: Vec<Span> = Vec::with_capacity(BATCH_MAX);
    while let Some(span) = rx.recv().await {
        batch.push(span);
        while batch.len() < BATCH_MAX {
            match rx.try_recv() {
                Ok(span) => batch.push(span),
                Err(_) => break,
            }
        }
        write(&state, run_id, &mut batch).await;
    }
}

async fn write(state: &AppState, run_id: Uuid, batch: &mut Vec<Span>) {
    for attempt in 1..=WRITE_ATTEMPTS {
        match repo::spans::insert(&state.db, batch).await {
            Ok(()) => {
                batch.clear();
                return;
            }
            Err(err) if attempt < WRITE_ATTEMPTS => {
                tracing::warn!(%run_id, error = %err, attempt, "span batch not written; retrying");
                tokio::time::sleep(Duration::from_millis(50 * u64::from(attempt))).await;
            }
            Err(err) => {
                tracing::error!(%run_id, error = %err, count = batch.len(),
                    "span batch not written; the run's trace will not verify");
            }
        }
    }
    batch.clear();
}
