//! The graph engine: editing, dependency detection, planning, scheduling
//! and execution.
#![forbid(unsafe_code)]

pub mod account;
pub mod analysis;
pub mod approvals;
pub mod artifacts;
pub mod assistant;
pub mod commands;
pub mod credentials;
pub mod defend;
pub mod deps;
pub mod editor;
pub mod executor;
pub mod guardrails;
pub mod json_stream;
pub mod knowledge;
pub mod planner;
pub mod policy;
pub mod recording;
pub mod risk;
pub mod scheduler;
pub mod summary;
pub mod templates;
pub mod trace;
pub mod transfer;
pub mod two_factor;
pub mod usage;
pub mod workspaces;

use std::sync::Arc;
use std::time::{Duration, Instant};

use dashmap::{DashMap, DashSet};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use commands::Attempt;

/// In-process engine bookkeeping shared by handlers and workers.
pub struct Engine {
    instance: Uuid,
    wake: Notify,
    active_runs: DashMap<Uuid, CancellationToken>,
    /// The nodes each agent is working on here, with what stops each of them.
    busy_agents: DashMap<Uuid, Vec<(Uuid, CancellationToken)>>,
    /// The node attempts open here over the runtime's gRPC stream, by run and node:
    /// where an operator's command to the attempt goes.
    attempts: DashMap<(Uuid, Uuid), Attempt>,
    /// Nodes started per agent, against a policy's `runs_per_hour`.
    dispatches: DashMap<Uuid, crate::dsa::token_bucket::TokenBucket>,
    deps_pending: DashSet<Uuid>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("instance", &self.instance)
            .field("active_runs", &self.active_runs.len())
            .field("attempts", &self.attempts.len())
            .finish_non_exhaustive()
    }
}

/// Keeps a node attempt registered while it is held; dropping it forgets the attempt.
pub struct AttemptGuard {
    engine: Arc<Engine>,
    key: (Uuid, Uuid),
}

impl Drop for AttemptGuard {
    fn drop(&mut self) {
        self.engine.attempts.remove(&self.key);
    }
}

impl Engine {
    /// Engine for this backend instance.
    pub fn new(instance: Uuid) -> Self {
        Engine {
            instance,
            wake: Notify::new(),
            active_runs: DashMap::new(),
            busy_agents: DashMap::new(),
            attempts: DashMap::new(),
            dispatches: DashMap::new(),
            deps_pending: DashSet::new(),
        }
    }

    /// Registers a node attempt open over the stream; it is forgotten when the guard drops.
    pub fn attempt_opened(self: &Arc<Self>, attempt: Attempt) -> AttemptGuard {
        let key = (attempt.run_id, attempt.node_id);
        self.attempts.insert(key, attempt);
        AttemptGuard {
            engine: self.clone(),
            key,
        }
    }

    /// The attempt of `node_id` in `run_id`, if it is open here.
    pub fn attempt(&self, run_id: Uuid, node_id: Uuid) -> Option<Attempt> {
        self.attempts.get(&(run_id, node_id)).map(|a| a.clone())
    }

    /// Identifier used when claiming runs.
    pub fn instance(&self) -> Uuid {
        self.instance
    }

    /// Wakes the local dispatcher (a run was queued).
    pub fn wake(&self) {
        self.wake.notify_one();
    }

    /// Resolves when [`Engine::wake`] is called.
    pub async fn woken(&self) {
        self.wake.notified().await;
    }

    /// Registers a run executing on this instance.
    pub fn register_run(&self, run_id: Uuid) -> CancellationToken {
        let token = CancellationToken::new();
        self.active_runs.insert(run_id, token.clone());
        token
    }

    /// Forgets a finished run.
    pub fn unregister_run(&self, run_id: Uuid) {
        self.active_runs.remove(&run_id);
    }

    /// Cancels a run if it executes on this instance; returns whether it did.
    pub fn cancel_local(&self, run_id: Uuid) -> bool {
        self.active_runs.get(&run_id).map(|t| t.cancel()).is_some()
    }

    /// Cancels every local run (shutdown).
    pub fn cancel_all(&self) {
        self.active_runs.iter().for_each(|t| t.cancel());
    }

    /// Marks an agent as working on `node`; cancelling `stop` ends that work.
    pub fn agent_started(&self, agent: Uuid, node: Uuid, stop: CancellationToken) {
        self.busy_agents
            .entry(agent)
            .or_default()
            .push((node, stop));
    }

    /// Marks an agent as finished with `node`.
    pub fn agent_finished(&self, agent: Uuid, node: Uuid) {
        self.busy_agents.remove_if_mut(&agent, |_, nodes| {
            nodes.retain(|(n, _)| *n != node);
            nodes.is_empty()
        });
    }

    /// Stops every node an agent is working on here (it was quarantined);
    /// returns how many.
    pub fn quarantine_local(&self, agent: Uuid) -> usize {
        self.busy_agents.get(&agent).map_or(0, |nodes| {
            nodes.iter().for_each(|(_, stop)| stop.cancel());
            nodes.len()
        })
    }

    /// Takes one of the `per_hour` node starts an agent is allowed, or says
    /// how long until the next one. The bucket is this instance's: with
    /// several instances the limit applies per instance.
    pub fn admit_dispatch(&self, agent: Uuid, per_hour: u32) -> Result<(), Duration> {
        let mut bucket = self.dispatches.entry(agent).or_insert_with(|| {
            crate::dsa::token_bucket::TokenBucket::new(
                per_hour.max(1),
                f64::from(per_hour) / 3600.0,
            )
        });
        bucket.try_acquire()
    }

    /// Forgets dispatch buckets that are full again.
    pub fn prune_dispatches(&self) {
        let now = Instant::now();
        self.dispatches.retain(|_, bucket| !bucket.is_idle(now));
    }

    /// Agents currently executing nodes on this instance.
    pub fn busy_agents(&self) -> Vec<Uuid> {
        self.busy_agents.iter().map(|e| *e.key()).collect()
    }

    /// Returns true if detection for `graph` was not already pending.
    pub(crate) fn mark_deps_pending(&self, graph: Uuid) -> bool {
        self.deps_pending.insert(graph)
    }

    pub(crate) fn clear_deps_pending(&self, graph: Uuid) {
        self.deps_pending.remove(&graph);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_runs_and_agents() {
        let e = Engine::new(Uuid::now_v7());
        let run = Uuid::now_v7();
        let token = e.register_run(run);
        assert!(e.cancel_local(run) && token.is_cancelled());
        e.unregister_run(run);
        assert!(!e.cancel_local(run));
        let agent = Uuid::now_v7();
        let (first, second) = (Uuid::now_v7(), Uuid::now_v7());
        let (stop_first, stop_second) = (CancellationToken::new(), CancellationToken::new());
        e.agent_started(agent, first, stop_first.clone());
        e.agent_started(agent, second, stop_second.clone());
        e.agent_finished(agent, first);
        assert_eq!(e.busy_agents(), vec![agent]);
        assert_eq!(e.quarantine_local(agent), 1, "the node still running stops");
        assert!(stop_second.is_cancelled() && !stop_first.is_cancelled());
        e.agent_finished(agent, second);
        assert!(e.busy_agents().is_empty());
        assert_eq!(e.quarantine_local(agent), 0);
        assert!(e.mark_deps_pending(run) && !e.mark_deps_pending(run));
    }

    #[test]
    fn attempts_are_known_while_their_guard_lives() {
        let e = Arc::new(Engine::new(Uuid::now_v7()));
        let (run, node) = (Uuid::now_v7(), Uuid::now_v7());
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let guard = e.attempt_opened(Attempt {
            run_id: run,
            node_id: node,
            graph_id: Uuid::now_v7(),
            agent_id: None,
            policy_version: 0,
            commands: tx,
            trace: trace::TraceHandle::discard(),
        });
        assert!(e.attempt(run, node).is_some());
        assert!(e.attempt(run, Uuid::now_v7()).is_none());
        assert!(format!("{e:?}").contains("attempts: 1"));
        drop(guard);
        assert!(e.attempt(run, node).is_none());
    }

    #[test]
    fn dispatches_are_rate_limited_per_agent() {
        let e = Engine::new(Uuid::now_v7());
        let agent = Uuid::now_v7();
        assert!(e.admit_dispatch(agent, 2).is_ok());
        assert!(e.admit_dispatch(agent, 2).is_ok());
        let wait = e.admit_dispatch(agent, 2).unwrap_err();
        assert!(wait > Duration::from_secs(60), "{wait:?}");
        assert!(e.admit_dispatch(Uuid::now_v7(), 2).is_ok(), "another agent");
        e.prune_dispatches();
        assert_eq!(e.dispatches.len(), 2, "buckets still refilling are kept");
    }
}
