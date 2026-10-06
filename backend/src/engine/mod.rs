//! The graph engine: editing, dependency detection, planning, scheduling
//! and execution.
#![forbid(unsafe_code)]

pub mod analysis;
pub mod artifacts;
pub mod assistant;
pub mod credentials;
pub mod deps;
pub mod editor;
pub mod executor;
pub mod guardrails;
pub mod json_stream;
pub mod knowledge;
pub mod planner;
pub mod scheduler;
pub mod templates;
pub mod usage;

use dashmap::{DashMap, DashSet};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// In-process engine bookkeeping shared by handlers and workers.
#[derive(Debug)]
pub struct Engine {
    instance: Uuid,
    wake: Notify,
    active_runs: DashMap<Uuid, CancellationToken>,
    busy_agents: DashMap<Uuid, usize>,
    deps_pending: DashSet<Uuid>,
}

impl Engine {
    /// Engine for this backend instance.
    pub fn new(instance: Uuid) -> Self {
        Engine {
            instance,
            wake: Notify::new(),
            active_runs: DashMap::new(),
            busy_agents: DashMap::new(),
            deps_pending: DashSet::new(),
        }
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

    /// Marks an agent as working on one more node.
    pub fn agent_started(&self, agent: Uuid) {
        *self.busy_agents.entry(agent).or_default() += 1;
    }

    /// Marks an agent as finished with one node.
    pub fn agent_finished(&self, agent: Uuid) {
        self.busy_agents.remove_if_mut(&agent, |_, n| {
            *n -= 1;
            *n == 0
        });
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
        e.agent_started(agent);
        e.agent_started(agent);
        e.agent_finished(agent);
        assert_eq!(e.busy_agents(), vec![agent]);
        e.agent_finished(agent);
        assert!(e.busy_agents().is_empty());
        assert!(e.mark_deps_pending(run) && !e.mark_deps_pending(run));
    }
}
