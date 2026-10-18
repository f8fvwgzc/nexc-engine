//! Run leases and fencing (docs/HA.md). The instance executing a run holds
//! a lease on it at an epoch. Every write the executor makes for the run
//! goes through [`begin`], which locks the run row and checks the epoch
//! first: once another instance took the run over, the former holder's
//! writes are refused, recorded once as `fenced`, and the former holder
//! stops its copy of the run.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sqlx::{Postgres, Transaction};
use tokio_util::sync::{CancellationToken, WaitForCancellationFuture};
use uuid::Uuid;

use crate::app::AppState;
use crate::repo;
use crate::repo::run_leases::{self, NewEvent};

/// The lease this instance holds on a run it executes. Cheap to clone.
#[derive(Clone, Debug)]
pub struct Lease {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    run_id: Uuid,
    /// `None`: writes are not fenced (a run nobody claims, such as an
    /// external agent's session on the Agent Firewall API).
    epoch: Option<i64>,
    instance: Uuid,
    /// Cancelled when the run must stop executing here.
    stop: CancellationToken,
    /// Writes are refused here without asking the database.
    lost: AtomicBool,
    /// The run stops because this instance shuts down: its trace is written
    /// and its lease released, so a peer takes it over at once.
    handing_off: AtomicBool,
    /// Whether a refused write was recorded already (once per lease).
    refusal_recorded: AtomicBool,
    /// When the database last confirmed the lease (the claim or a renewal).
    confirmed: Mutex<Instant>,
}

impl Lease {
    /// A lease held at `epoch` by `instance`.
    pub fn held(run_id: Uuid, epoch: i64, instance: Uuid) -> Self {
        Self::new(run_id, Some(epoch), instance)
    }

    /// A lease that fences nothing (runs that are not claimed by a dispatcher).
    pub fn unfenced(run_id: Uuid) -> Self {
        Self::new(run_id, None, Uuid::nil())
    }

    fn new(run_id: Uuid, epoch: Option<i64>, instance: Uuid) -> Self {
        Lease {
            inner: Arc::new(Inner {
                run_id,
                epoch,
                instance,
                stop: CancellationToken::new(),
                lost: AtomicBool::new(false),
                handing_off: AtomicBool::new(false),
                refusal_recorded: AtomicBool::new(false),
                confirmed: Mutex::new(Instant::now()),
            }),
        }
    }

    /// The database renewed the lease just now.
    pub fn confirmed(&self) {
        *self.inner.confirmed.lock().expect("lease clock") = Instant::now();
    }

    /// How long since the database last confirmed the lease.
    pub fn unconfirmed_for(&self) -> Duration {
        self.inner.confirmed.lock().expect("lease clock").elapsed()
    }

    pub fn run_id(&self) -> Uuid {
        self.inner.run_id
    }

    /// The epoch held; `None` for an unfenced lease.
    pub fn epoch(&self) -> Option<i64> {
        self.inner.epoch
    }

    pub fn instance(&self) -> Uuid {
        self.inner.instance
    }

    /// Whether the two are the same lease (not only the same run).
    pub fn same(&self, other: &Lease) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    /// Resolves when the run must stop executing here.
    pub fn stopped(&self) -> WaitForCancellationFuture<'_> {
        self.inner.stop.cancelled()
    }

    /// Whether the run must stop executing here.
    pub fn is_stopped(&self) -> bool {
        self.inner.stop.is_cancelled()
    }

    /// Whether writes are refused here: the lease moved on.
    pub fn is_lost(&self) -> bool {
        self.inner.lost.load(Ordering::SeqCst)
    }

    /// Whether the run stops because this instance hands it to a peer.
    pub fn is_handing_off(&self) -> bool {
        self.inner.handing_off.load(Ordering::SeqCst)
    }

    /// The lease moved on (or can no longer be trusted): nothing more is
    /// written for the run from here, and its execution stops.
    pub fn lose(&self) {
        self.inner.lost.store(true, Ordering::SeqCst);
        self.inner.stop.cancel();
    }

    /// This instance shuts down: the run stops, its trace is written and
    /// its lease released (see `scheduler::hand_off`).
    pub fn hand_off(&self) {
        self.inner.handing_off.store(true, Ordering::SeqCst);
        self.inner.stop.cancel();
    }
}

/// Why a write of the executor did not happen.
#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    /// The run changed hands (or ended): the write was refused.
    #[error("refused: this instance no longer holds the run's lease")]
    Fenced,
    #[error(transparent)]
    Db(#[from] sqlx::Error),
}

impl WriteError {
    pub fn is_fenced(&self) -> bool {
        matches!(self, WriteError::Fenced)
    }
}

/// Opens a transaction for a write of the run's executor, fenced by the
/// lease: the run row is locked and its epoch checked first. `what` names
/// the write for the record when it is refused.
pub async fn begin(
    state: &AppState,
    lease: &Lease,
    what: &str,
) -> Result<Transaction<'static, Postgres>, WriteError> {
    // Known lost here already: refused without asking (and without a record:
    // this instance stopped the run itself, nothing was attempted against it).
    if lease.is_lost() {
        return Err(WriteError::Fenced);
    }
    let mut tx = state.db.begin().await?;
    let Some(held) = lease.epoch() else {
        return Ok(tx);
    };
    let current = repo::runs::lock_lease(&mut *tx, lease.run_id()).await?;
    if current == Some(held) {
        return Ok(tx);
    }
    let _ = tx.rollback().await;
    refused(state, lease, what, current).await;
    Err(WriteError::Fenced)
}

/// Records a write the database refused to a former holder (once per
/// lease, counted every time) and gives the lease up. `current` is the
/// run's epoch now, or `None` when the run is no longer running.
async fn refused(state: &AppState, lease: &Lease, what: &str, current: Option<i64>) {
    lease.lose();
    state
        .metrics
        .run_leases
        .add(&[("event", run_leases::FENCED)], 1);
    let Some(held) = lease.epoch() else {
        return;
    };
    if lease.inner.refusal_recorded.swap(true, Ordering::SeqCst) {
        return;
    }
    let detail = match current {
        Some(epoch) => format!("{what} refused: the run is at epoch {epoch}"),
        None => format!("{what} refused: the run is no longer running"),
    };
    tracing::warn!(run_id = %lease.run_id(), epoch = held, %detail, "write of a former lease holder refused");
    if let Err(err) = run_leases::record(
        &state.db,
        NewEvent {
            run_id: lease.run_id(),
            epoch: held,
            instance: lease.instance(),
            kind: run_leases::FENCED,
            previous: None,
            detail: &detail,
        },
    )
    .await
    {
        tracing::error!(run_id = %lease.run_id(), error = %err, "refused write not recorded");
    }
}

/// Records an event of the lease (claimed, taken over, released, lost);
/// failures are logged, never fatal.
pub async fn record(
    state: &AppState,
    lease: &Lease,
    kind: &str,
    epoch: i64,
    previous: Option<Uuid>,
    detail: &str,
) {
    state.metrics.run_leases.add(&[("event", kind)], 1);
    if let Err(err) = run_leases::record(
        &state.db,
        NewEvent {
            run_id: lease.run_id(),
            epoch,
            instance: lease.instance(),
            kind,
            previous,
            detail,
        },
    )
    .await
    {
        tracing::error!(run_id = %lease.run_id(), error = %err, kind, "lease event not recorded");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn losing_stops_and_handing_off_stops_without_losing() {
        let run = Uuid::now_v7();
        let lease = Lease::held(run, 3, Uuid::now_v7());
        assert_eq!((lease.run_id(), lease.epoch()), (run, Some(3)));
        let copy = lease.clone();
        assert!(copy.same(&lease) && !Lease::held(run, 3, lease.instance()).same(&lease));
        lease.hand_off();
        assert!(copy.is_stopped() && copy.is_handing_off() && !copy.is_lost());
        let other = Lease::held(run, 4, Uuid::now_v7());
        other.lose();
        assert!(other.is_stopped() && other.is_lost() && !other.is_handing_off());
        assert_eq!(Lease::unfenced(run).epoch(), None);
        assert!(WriteError::Fenced.is_fenced());
    }
}
