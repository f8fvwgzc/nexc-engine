//! The per-run trace writer. Executors emit span drafts; one task per run
//! gives each its place in the run's hash chain in order of receipt and
//! fans it out at once as a `trace.span` event; a second task writes the
//! chained spans to the database in batches, fenced by the run's lease (a
//! former holder's spans are refused). Nothing here is on the hot path of
//! the work being traced: emitting is a channel send. A run taken over by
//! another instance continues its chain from the last span written
//! ([`Tail`]), so it verifies as one chain.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use uuid::Uuid;

use super::lease::{self, Lease, WriteError};
use crate::app::AppState;
use crate::domain::trace::{self, GENESIS, Span, SpanDraft};
use crate::realtime::events::SseEvent;
use crate::repo;

/// Spans written in one statement at most.
const BATCH_MAX: usize = 500;
/// How long a batch is retried while the database does not take it (a
/// failover) before it is given up.
const WRITE_PATIENCE: Duration = Duration::from_secs(120);
/// The longest pause between two tries of a batch.
const WRITE_BACKOFF_MAX: Duration = Duration::from_secs(2);

/// Where a run's chain stands when its trace is opened: nothing yet, or the
/// last span a former holder of the run wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tail {
    /// The position the next span takes.
    pub seq: i32,
    /// The hash the next span links to.
    pub prev: [u8; 32],
}

impl Tail {
    /// A chain with no span yet.
    pub const GENESIS: Tail = Tail {
        seq: 0,
        prev: GENESIS,
    };

    /// The chain of `run_id` as written so far.
    pub async fn of_run(state: &AppState, run_id: Uuid) -> Result<Tail, sqlx::Error> {
        Ok(match repo::spans::tail(&state.db, run_id).await? {
            Some((seq, hash)) => Tail {
                seq: seq + 1,
                prev: hash,
            },
            None => Tail::GENESIS,
        })
    }
}

struct Pending {
    draft: SpanDraft,
    emitted: Instant,
}

/// What travels to a run's chain: a span, or the order to close it.
enum Message {
    Span(Pending),
    /// Everything sent before this is chained; nothing after it is.
    Seal,
}

/// Emits spans into one run's chain. Cheap to clone. Emitting never blocks
/// and never fails the work being traced.
#[derive(Clone)]
pub struct TraceHandle {
    tx: mpsc::UnboundedSender<Message>,
}

impl TraceHandle {
    /// Appends a span to the run's chain (asynchronously).
    pub fn emit(&self, draft: SpanDraft) {
        let pending = Pending {
            draft,
            emitted: Instant::now(),
        };
        if self.tx.send(Message::Span(pending)).is_err() {
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

/// Opens the trace of `run_id` under `lease`, continuing the chain at `tail`.
pub fn start(state: AppState, run_id: Uuid, graph_id: Uuid, lease: Lease, tail: Tail) -> TraceSink {
    let (tx, rx) = mpsc::unbounded_channel();
    let (persist_tx, persist_rx) = mpsc::unbounded_channel();
    let persister = tokio::spawn(persist(state.clone(), lease, persist_rx));
    let chain = tokio::spawn(chain(
        state, run_id, graph_id, tail, rx, persist_tx, persister,
    ));
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
    /// before this returns. Spans emitted afterwards are dropped, and
    /// handles still held elsewhere (a background task) do not hold it open.
    pub async fn seal(self) -> Sealed {
        if self.handle.tx.send(Message::Seal).is_err() {
            tracing::error!("trace writer stopped before the run was sealed");
        }
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
    tail: Tail,
    mut rx: mpsc::UnboundedReceiver<Message>,
    persist_tx: mpsc::UnboundedSender<Span>,
    persister: JoinHandle<()>,
) -> Sealed {
    let mut prev = tail.prev;
    let mut seq: i32 = tail.seq;
    let mut seen: HashSet<Uuid> = HashSet::new();
    while let Some(Message::Span(Pending { mut draft, emitted })) = rx.recv().await {
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
    // Later spans find the channel closed and are dropped.
    rx.close();
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
async fn persist(state: AppState, lease: Lease, mut rx: mpsc::UnboundedReceiver<Span>) {
    let mut batch: Vec<Span> = Vec::with_capacity(BATCH_MAX);
    while let Some(span) = rx.recv().await {
        batch.push(span);
        while batch.len() < BATCH_MAX {
            match rx.try_recv() {
                Ok(span) => batch.push(span),
                Err(_) => break,
            }
        }
        write(&state, &lease, &mut batch).await;
    }
}

/// Writes one batch under the lease. A database that does not take it (a
/// failover) is tried again with back-off for a while; a refused batch (the
/// run changed hands) is dropped, and so is every later one.
async fn write(state: &AppState, lease: &Lease, batch: &mut Vec<Span>) {
    let run_id = lease.run_id();
    let deadline = Instant::now() + WRITE_PATIENCE;
    let mut pause = Duration::from_millis(50);
    let mut attempt = 0;
    loop {
        attempt += 1;
        match insert(state, lease, batch).await {
            Ok(()) => break,
            Err(WriteError::Fenced) => {
                tracing::debug!(%run_id, count = batch.len(), "span batch of a former lease holder dropped");
                break;
            }
            Err(WriteError::Db(err)) if Instant::now() < deadline => {
                tracing::warn!(%run_id, error = %err, attempt, "span batch not written; retrying");
                tokio::time::sleep(pause).await;
                pause = (pause * 2).min(WRITE_BACKOFF_MAX);
            }
            Err(WriteError::Db(err)) => {
                tracing::error!(%run_id, error = %err, count = batch.len(),
                    "span batch not written; the run's trace will not verify");
                break;
            }
        }
    }
    batch.clear();
}

async fn insert(state: &AppState, lease: &Lease, batch: &[Span]) -> Result<(), WriteError> {
    let mut tx = lease::begin(state, lease, "trace spans").await?;
    repo::spans::insert(&mut *tx, batch).await?;
    tx.commit().await?;
    Ok(())
}
